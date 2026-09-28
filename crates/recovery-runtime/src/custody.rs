use connectivity_core::{Bundle, BundlePriority, DtnQueue};
use peer_session::{SecureSession, SessionError};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

pub const KIND_CUSTODY_OFFER: u8 = 0x30;
pub const KIND_CUSTODY_ACK: u8 = 0x31;
pub const MAX_CUSTODY_PAYLOAD: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CustodyStatus {
    Accepted = 0,
    AlreadyHave = 1,
    RejectedExpired = 2,
}

impl TryFrom<u8> for CustodyStatus {
    type Error = CustodyError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Accepted),
            1 => Ok(Self::AlreadyHave),
            2 => Ok(Self::RejectedExpired),
            other => Err(CustodyError::InvalidStatus(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustodySendOutcome {
    Empty,
    Transferred {
        bundle_id: u64,
        status: CustodyStatus,
    },
    Retained {
        bundle_id: u64,
        status: CustodyStatus,
    },
}

#[derive(Debug)]
pub enum CustodyError {
    Session(SessionError),
    Truncated,
    PayloadTooLarge,
    InvalidPriority(u8),
    InvalidStatus(u8),
    UnexpectedFrameKind(u8),
    BundleIdMismatch { expected: u64, got: u64 },
    TimeOverflow,
    DuplicateInvariant(u64),
    TrailingBytes,
}

impl From<SessionError> for CustodyError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CustodyOffer {
    id: u64,
    priority: BundlePriority,
    remaining_ttl_ms: u64,
    attempts: u32,
    payload: Vec<u8>,
}

pub fn offer_next_bundle<S: Read + Write>(
    queue: &mut DtnQueue,
    session: &mut SecureSession,
    stream: &mut S,
    now: Instant,
) -> Result<CustodySendOutcome, CustodyError> {
    let offer = queue.next_for_send(now).map(|bundle| {
        let elapsed = now.saturating_duration_since(bundle.created_at);
        let remaining = bundle.ttl.saturating_sub(elapsed);
        (bundle.id, bundle.priority, remaining, bundle.attempts, bundle.payload.clone())
    });

    let Some((bundle_id, priority, remaining, attempts, payload)) = offer else {
        return Ok(CustodySendOutcome::Empty);
    };

    let remaining_ttl_ms = u64::try_from(remaining.as_millis())
        .map_err(|_| CustodyError::TimeOverflow)?;

    let offer = CustodyOffer {
        id: bundle_id,
        priority,
        remaining_ttl_ms,
        attempts,
        payload,
    };
    let payload = encode_offer(&offer)?;
    session.send(stream, KIND_CUSTODY_OFFER, &payload)?;

    let (kind, ack_payload) = session.receive(stream)?;
    if kind != KIND_CUSTODY_ACK {
        return Err(CustodyError::UnexpectedFrameKind(kind));
    }

    let (ack_id, status) = decode_ack(&ack_payload)?;
    if ack_id != bundle_id {
        return Err(CustodyError::BundleIdMismatch {
            expected: bundle_id,
            got: ack_id,
        });
    }

    match status {
        CustodyStatus::Accepted | CustodyStatus::AlreadyHave => {
            if !queue.ack(bundle_id) {
                return Err(CustodyError::DuplicateInvariant(bundle_id));
            }

            Ok(CustodySendOutcome::Transferred {
                bundle_id,
                status,
            })
        }
        CustodyStatus::RejectedExpired => Ok(CustodySendOutcome::Retained {
            bundle_id,
            status,
        }),
    }
}

pub fn receive_one_bundle<S: Read + Write>(
    queue: &mut DtnQueue,
    session: &mut SecureSession,
    stream: &mut S,
    now: Instant,
) -> Result<(u64, CustodyStatus), CustodyError> {
    let (kind, payload) = session.receive(stream)?;
    if kind != KIND_CUSTODY_OFFER {
        return Err(CustodyError::UnexpectedFrameKind(kind));
    }

    let offer = decode_offer(&payload)?;

    let status = if offer.remaining_ttl_ms == 0 {
        CustodyStatus::RejectedExpired
    } else if queue.contains(offer.id) {
        CustodyStatus::AlreadyHave
    } else {
        let inserted = queue.push_unique(Bundle {
            id: offer.id,
            priority: offer.priority,
            created_at: now,
            ttl: Duration::from_millis(offer.remaining_ttl_ms),
            payload: offer.payload,
            attempts: offer.attempts,
        });

        if !inserted {
            return Err(CustodyError::DuplicateInvariant(offer.id));
        }

        CustodyStatus::Accepted
    };

    let ack = encode_ack(offer.id, status);
    session.send(stream, KIND_CUSTODY_ACK, &ack)?;

    Ok((offer.id, status))
}

fn encode_offer(offer: &CustodyOffer) -> Result<Vec<u8>, CustodyError> {
    if offer.payload.len() > MAX_CUSTODY_PAYLOAD {
        return Err(CustodyError::PayloadTooLarge);
    }

    let mut out = Vec::with_capacity(25 + offer.payload.len());
    out.extend_from_slice(&offer.id.to_be_bytes());
    out.push(encode_priority(offer.priority));
    out.extend_from_slice(&offer.remaining_ttl_ms.to_be_bytes());
    out.extend_from_slice(&offer.attempts.to_be_bytes());
    out.extend_from_slice(&(offer.payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&offer.payload);
    Ok(out)
}

fn decode_offer(bytes: &[u8]) -> Result<CustodyOffer, CustodyError> {
    const FIXED: usize = 25;
    if bytes.len() < FIXED {
        return Err(CustodyError::Truncated);
    }

    let mut cursor = 0;

    let id = u64::from_be_bytes(
        bytes[cursor..cursor + 8]
            .try_into()
            .map_err(|_| CustodyError::Truncated)?,
    );
    cursor += 8;

    let priority = decode_priority(bytes[cursor])?;
    cursor += 1;

    let remaining_ttl_ms = u64::from_be_bytes(
        bytes[cursor..cursor + 8]
            .try_into()
            .map_err(|_| CustodyError::Truncated)?,
    );
    cursor += 8;

    let attempts = u32::from_be_bytes(
        bytes[cursor..cursor + 4]
            .try_into()
            .map_err(|_| CustodyError::Truncated)?,
    );
    cursor += 4;

    let payload_len = u32::from_be_bytes(
        bytes[cursor..cursor + 4]
            .try_into()
            .map_err(|_| CustodyError::Truncated)?,
    ) as usize;
    cursor += 4;

    if payload_len > MAX_CUSTODY_PAYLOAD {
        return Err(CustodyError::PayloadTooLarge);
    }

    let payload = bytes
        .get(cursor..cursor + payload_len)
        .ok_or(CustodyError::Truncated)?
        .to_vec();
    cursor += payload_len;

    if cursor != bytes.len() {
        return Err(CustodyError::TrailingBytes);
    }

    Ok(CustodyOffer {
        id,
        priority,
        remaining_ttl_ms,
        attempts,
        payload,
    })
}

fn encode_ack(id: u64, status: CustodyStatus) -> [u8; 9] {
    let mut out = [0_u8; 9];
    out[..8].copy_from_slice(&id.to_be_bytes());
    out[8] = status as u8;
    out
}

fn decode_ack(bytes: &[u8]) -> Result<(u64, CustodyStatus), CustodyError> {
    if bytes.len() != 9 {
        return Err(CustodyError::Truncated);
    }

    let id = u64::from_be_bytes(
        bytes[..8]
            .try_into()
            .map_err(|_| CustodyError::Truncated)?,
    );
    let status = CustodyStatus::try_from(bytes[8])?;
    Ok((id, status))
}

fn encode_priority(priority: BundlePriority) -> u8 {
    match priority {
        BundlePriority::Bulk => 0,
        BundlePriority::Normal => 1,
        BundlePriority::Urgent => 2,
    }
}

fn decode_priority(value: u8) -> Result<BundlePriority, CustodyError> {
    match value {
        0 => Ok(BundlePriority::Bulk),
        1 => Ok(BundlePriority::Normal),
        2 => Ok(BundlePriority::Urgent),
        other => Err(CustodyError::InvalidPriority(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use peer_session::{
        perform_client_handshake, perform_server_handshake,
        NonceReplayCache, PeerKey,
    };
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn bundle(now: Instant, id: u64) -> Bundle {
        Bundle {
            id,
            priority: BundlePriority::Urgent,
            created_at: now,
            ttl: Duration::from_secs(300),
            payload: vec![1, 2, 3, 4],
            attempts: 0,
        }
    }

    #[test]
    fn custody_moves_bundle_only_after_authenticated_ack() {
        let now = Instant::now();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let key = PeerKey::new([0x91; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 2, &key, &mut replay)
                    .unwrap();

            let mut queue = DtnQueue::new();
            let (id, status) = receive_one_bundle(
                &mut queue,
                &mut session,
                &mut stream,
                Instant::now(),
            )
            .unwrap();

            assert_eq!(id, 99);
            assert_eq!(status, CustodyStatus::Accepted);
            queue
        });

        let mut source = DtnQueue::new();
        assert!(source.push_unique(bundle(now, 99)));

        let mut stream = TcpStream::connect(address).unwrap();
        let key = PeerKey::new([0x91; 32]);
        let (_, mut session) =
            perform_client_handshake(&mut stream, 1, &key).unwrap();

        let outcome =
            offer_next_bundle(&mut source, &mut session, &mut stream, now)
                .unwrap();

        assert_eq!(
            outcome,
            CustodySendOutcome::Transferred {
                bundle_id: 99,
                status: CustodyStatus::Accepted,
            }
        );
        assert!(source.is_empty());

        let receiver_queue = receiver.join().unwrap();
        assert_eq!(receiver_queue.len(), 1);
        assert!(receiver_queue.contains(99));
        assert_eq!(
            receiver_queue.iter().next().unwrap().payload,
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn already_present_bundle_is_deduplicated_and_acked() {
        let now = Instant::now();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let key = PeerKey::new([0x92; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 2, &key, &mut replay)
                    .unwrap();

            let mut queue = DtnQueue::new();
            assert!(queue.push_unique(bundle(Instant::now(), 77)));

            let (_, status) = receive_one_bundle(
                &mut queue,
                &mut session,
                &mut stream,
                Instant::now(),
            )
            .unwrap();

            assert_eq!(status, CustodyStatus::AlreadyHave);
            assert_eq!(queue.len(), 1);
        });

        let mut source = DtnQueue::new();
        assert!(source.push_unique(bundle(now, 77)));

        let mut stream = TcpStream::connect(address).unwrap();
        let key = PeerKey::new([0x92; 32]);
        let (_, mut session) =
            perform_client_handshake(&mut stream, 1, &key).unwrap();

        let outcome =
            offer_next_bundle(&mut source, &mut session, &mut stream, now)
                .unwrap();

        assert_eq!(
            outcome,
            CustodySendOutcome::Transferred {
                bundle_id: 77,
                status: CustodyStatus::AlreadyHave,
            }
        );
        assert!(source.is_empty());

        receiver.join().unwrap();
    }
}
