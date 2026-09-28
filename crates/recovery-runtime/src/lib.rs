pub mod custody;
pub mod spool;

pub use custody::{
    offer_next_bundle, receive_one_bundle, CustodyError, CustodySendOutcome,
    CustodyStatus, KIND_CUSTODY_ACK, KIND_CUSTODY_OFFER,
};
pub use spool::{decode_queue, load_queue, save_queue, SpoolError};

use connectivity_core::{
    Bundle, BundlePriority, DtnQueue,
};
use peer_egress::{
    decode_request, decode_response, encode_request, ProtocolError,
    ResolveRequest, ResolveStatus, DEFAULT_RELAY_BUDGET,
    KIND_RESOLVE_REQUEST, KIND_RESOLVE_RESPONSE,
};
use peer_session::{SecureSession, SessionError};
use std::io::{Read, Write};
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum RuntimeError {
    Protocol(ProtocolError),
    Session(SessionError),
    UnexpectedFrameKind(u8),
    RequestIdMismatch { expected: u32, got: u32 },
    CorruptQueuedBundle,
    DuplicateBundle(u64),
}

impl From<ProtocolError> for RuntimeError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<SessionError> for RuntimeError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    Empty,
    Delivered {
        bundle_id: u64,
        request_id: u32,
        addresses: Vec<IpAddr>,
    },
    RemoteRejected {
        bundle_id: u64,
        request_id: u32,
        status: ResolveStatus,
    },
}

/// Queues a constrained semantic DNS request without requiring a live route.
///
/// The bundle can remain in the local DTN queue until a suitable authenticated
/// peer contact appears later.
pub fn enqueue_resolve(
    queue: &mut DtnQueue,
    bundle_id: u64,
    request_id: u32,
    hostname: &str,
    priority: BundlePriority,
    ttl: Duration,
    now: Instant,
) -> Result<(), RuntimeError> {
    let request = ResolveRequest {
        request_id,
        relays_remaining: DEFAULT_RELAY_BUDGET,
        hostname: hostname.to_owned(),
    };
    let payload = encode_request(&request)?;

    let inserted = queue.push_unique(Bundle {
        id: bundle_id,
        priority,
        created_at: now,
        ttl,
        payload,
        attempts: 0,
    });

    if !inserted {
        return Err(RuntimeError::DuplicateBundle(bundle_id));
    }

    Ok(())
}

/// Attempts one queued request over an already-authenticated peer contact.
///
/// Transport/protocol failures leave the bundle queued for a future contact.
/// A syntactically valid remote response is terminal and removes the bundle,
/// including explicit remote rejections such as "no public address".
pub fn dispatch_next_resolve<S: Read + Write>(
    queue: &mut DtnQueue,
    session: &mut SecureSession,
    stream: &mut S,
    now: Instant,
) -> Result<DispatchOutcome, RuntimeError> {
    let next = queue
        .next_for_send(now)
        .map(|bundle| (bundle.id, bundle.payload.clone()));

    let Some((bundle_id, payload)) = next else {
        return Ok(DispatchOutcome::Empty);
    };

    let request = match decode_request(&payload) {
        Ok(request) => request,
        Err(error) => {
            // A locally corrupt queued payload cannot become valid by retrying
            // another peer. Drop it so it cannot permanently poison the queue.
            queue.ack(bundle_id);
            return Err(RuntimeError::Protocol(error));
        }
    };

    session.send(stream, KIND_RESOLVE_REQUEST, &payload)?;

    let (kind, response_payload) = session.receive(stream)?;
    if kind != KIND_RESOLVE_RESPONSE {
        return Err(RuntimeError::UnexpectedFrameKind(kind));
    }

    let response = decode_response(&response_payload)?;
    if response.request_id != request.request_id {
        return Err(RuntimeError::RequestIdMismatch {
            expected: request.request_id,
            got: response.request_id,
        });
    }

    if !queue.ack(bundle_id) {
        return Err(RuntimeError::CorruptQueuedBundle);
    }

    if response.status == ResolveStatus::Ok {
        Ok(DispatchOutcome::Delivered {
            bundle_id,
            request_id: request.request_id,
            addresses: response.addresses,
        })
    } else {
        Ok(DispatchOutcome::RemoteRejected {
            bundle_id,
            request_id: request.request_id,
            status: response.status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use peer_egress::{serve_one, Resolver};
    use peer_session::{
        perform_client_handshake, perform_server_handshake,
        NonceReplayCache, PeerKey,
    };
    use std::io;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    struct FakeResolver {
        addresses: Vec<IpAddr>,
    }

    impl Resolver for FakeResolver {
        fn resolve(&self, _hostname: &str) -> io::Result<Vec<IpAddr>> {
            Ok(self.addresses.clone())
        }
    }


    #[test]
    fn duplicate_bundle_id_is_rejected_at_runtime_boundary() {
        let now = Instant::now();
        let mut queue = DtnQueue::new();

        enqueue_resolve(
            &mut queue,
            500,
            1,
            "example.com",
            BundlePriority::Normal,
            Duration::from_secs(60),
            now,
        )
        .unwrap();

        let duplicate = enqueue_resolve(
            &mut queue,
            500,
            2,
            "example.org",
            BundlePriority::Urgent,
            Duration::from_secs(60),
            now,
        );

        assert!(matches!(
            duplicate,
            Err(RuntimeError::DuplicateBundle(500))
        ));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn queued_request_survives_until_contact_and_then_delivers() {
        let queued_at = Instant::now();
        let mut queue = DtnQueue::new();

        enqueue_resolve(
            &mut queue,
            9001,
            77,
            "example.com",
            BundlePriority::Urgent,
            Duration::from_secs(3600),
            queued_at,
        )
        .unwrap();

        assert_eq!(queue.len(), 1);

        // No transport operation happened above. Only now does a peer contact
        // appear and an authenticated session become available.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let key = PeerKey::new([0x66; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 200, &key, &mut replay)
                    .unwrap();

            let resolver = FakeResolver {
                addresses: vec![
                    "10.0.0.1".parse().unwrap(),
                    "8.8.8.8".parse().unwrap(),
                ],
            };

            serve_one(&mut session, &mut stream, &resolver).unwrap();
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let key = PeerKey::new([0x66; 32]);
        let (_, mut session) =
            perform_client_handshake(&mut stream, 100, &key).unwrap();

        let outcome = dispatch_next_resolve(
            &mut queue,
            &mut session,
            &mut stream,
            queued_at + Duration::from_secs(5),
        )
        .unwrap();

        assert_eq!(
            outcome,
            DispatchOutcome::Delivered {
                bundle_id: 9001,
                request_id: 77,
                addresses: vec!["8.8.8.8".parse().unwrap()],
            }
        );
        assert!(queue.is_empty());

        server.join().unwrap();
    }


    #[test]
    fn store_carry_forward_request_and_return_result_across_contacts() {
        use peer_egress::{decode_response, encode_response, ResolveResponse};
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let t0 = Instant::now();
        let wall0 = UNIX_EPOCH + Duration::from_secs(2_000_000_000);

        // Phase 1: A is offline and creates work without any live route.
        let mut a_queue = DtnQueue::new();
        enqueue_resolve(
            &mut a_queue,
            1001,
            501,
            "example.com",
            BundlePriority::Urgent,
            Duration::from_secs(3600),
            t0,
        )
        .unwrap();

        // Phase 2: A later meets B. B has no egress yet, but accepts custody.
        let custody_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let custody_addr = custody_listener.local_addr().unwrap();

        let b_contact = thread::spawn(move || {
            let (mut stream, _) = custody_listener.accept().unwrap();
            let key = PeerKey::new([0xA1; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 200, &key, &mut replay)
                    .unwrap();

            let mut b_queue = DtnQueue::new();
            let (_, status) = receive_one_bundle(
                &mut b_queue,
                &mut session,
                &mut stream,
                Instant::now(),
            )
            .unwrap();

            assert_eq!(status, CustodyStatus::Accepted);
            b_queue
        });

        let mut custody_stream = TcpStream::connect(custody_addr).unwrap();
        let key = PeerKey::new([0xA1; 32]);
        let (_, mut custody_session) =
            perform_client_handshake(&mut custody_stream, 100, &key).unwrap();

        let custody_outcome = offer_next_bundle(
            &mut a_queue,
            &mut custody_session,
            &mut custody_stream,
            t0,
        )
        .unwrap();

        assert!(matches!(
            custody_outcome,
            CustodySendOutcome::Transferred {
                bundle_id: 1001,
                status: CustodyStatus::Accepted,
            }
        ));
        assert!(a_queue.is_empty());

        let b_queue = b_contact.join().unwrap();

        // Phase 3: B can shut down/restart before ever seeing an egress node.
        let spool_path = std::env::temp_dir().join(format!(
            "sanpham3-g6-roundtrip-{}-{}.bin",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));

        save_queue(&spool_path, &b_queue, Instant::now(), wall0).unwrap();
        let mut b_queue = load_queue(
            &spool_path,
            Instant::now(),
            wall0 + Duration::from_secs(2),
        )
        .unwrap();
        fs::remove_file(&spool_path).unwrap();

        assert!(b_queue.contains(1001));

        // Phase 4: later B meets C. Only C has an egress resolver.
        let egress_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let egress_addr = egress_listener.local_addr().unwrap();

        let c_egress = thread::spawn(move || {
            let (mut stream, _) = egress_listener.accept().unwrap();
            let key = PeerKey::new([0xA2; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 300, &key, &mut replay)
                    .unwrap();

            let resolver = FakeResolver {
                addresses: vec![
                    "10.1.2.3".parse().unwrap(),
                    "8.8.8.8".parse().unwrap(),
                ],
            };

            serve_one(&mut session, &mut stream, &resolver).unwrap();
        });

        let mut egress_stream = TcpStream::connect(egress_addr).unwrap();
        let key = PeerKey::new([0xA2; 32]);
        let (_, mut egress_session) =
            perform_client_handshake(&mut egress_stream, 200, &key).unwrap();

        let delivered = dispatch_next_resolve(
            &mut b_queue,
            &mut egress_session,
            &mut egress_stream,
            Instant::now(),
        )
        .unwrap();

        let addresses = match delivered {
            DispatchOutcome::Delivered {
                bundle_id,
                request_id,
                addresses,
            } => {
                assert_eq!(bundle_id, 1001);
                assert_eq!(request_id, 501);
                addresses
            }
            other => panic!("unexpected egress outcome: {other:?}"),
        };
        assert!(b_queue.is_empty());
        c_egress.join().unwrap();

        // Phase 5: B stores the result as another DTN bundle and carries it
        // back to A during a completely separate contact.
        let response_payload = encode_response(&ResolveResponse {
            request_id: 501,
            status: ResolveStatus::Ok,
            addresses: addresses.clone(),
        })
        .unwrap();

        let mut b_return_queue = DtnQueue::new();
        assert!(b_return_queue.push_unique(Bundle {
            id: 2001,
            priority: BundlePriority::Urgent,
            created_at: Instant::now(),
            ttl: Duration::from_secs(3600),
            payload: response_payload,
            attempts: 0,
        }));

        let return_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let return_addr = return_listener.local_addr().unwrap();

        let a_return = thread::spawn(move || {
            let (mut stream, _) = return_listener.accept().unwrap();
            let key = PeerKey::new([0xA3; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 100, &key, &mut replay)
                    .unwrap();

            let mut returned = DtnQueue::new();
            receive_one_bundle(
                &mut returned,
                &mut session,
                &mut stream,
                Instant::now(),
            )
            .unwrap();
            returned
        });

        let mut return_stream = TcpStream::connect(return_addr).unwrap();
        let key = PeerKey::new([0xA3; 32]);
        let (_, mut return_session) =
            perform_client_handshake(&mut return_stream, 200, &key).unwrap();

        let return_outcome = offer_next_bundle(
            &mut b_return_queue,
            &mut return_session,
            &mut return_stream,
            Instant::now(),
        )
        .unwrap();

        assert!(matches!(
            return_outcome,
            CustodySendOutcome::Transferred {
                bundle_id: 2001,
                status: CustodyStatus::Accepted,
            }
        ));

        let returned = a_return.join().unwrap();
        let result_bundle = returned.iter().next().unwrap();
        let response = decode_response(&result_bundle.payload).unwrap();

        assert_eq!(response.request_id, 501);
        assert_eq!(response.status, ResolveStatus::Ok);
        assert_eq!(response.addresses, addresses);
    }

    #[test]
    fn valid_remote_rejection_is_terminal_and_acked() {
        let now = Instant::now();
        let mut queue = DtnQueue::new();

        enqueue_resolve(
            &mut queue,
            11,
            12,
            "example.com",
            BundlePriority::Normal,
            Duration::from_secs(60),
            now,
        )
        .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let key = PeerKey::new([0x33; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 2, &key, &mut replay)
                    .unwrap();

            let resolver = FakeResolver {
                addresses: vec!["192.168.1.5".parse().unwrap()],
            };
            serve_one(&mut session, &mut stream, &resolver).unwrap();
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let key = PeerKey::new([0x33; 32]);
        let (_, mut session) =
            perform_client_handshake(&mut stream, 1, &key).unwrap();

        let outcome =
            dispatch_next_resolve(&mut queue, &mut session, &mut stream, now)
                .unwrap();

        assert_eq!(
            outcome,
            DispatchOutcome::RemoteRejected {
                bundle_id: 11,
                request_id: 12,
                status: ResolveStatus::NoPublicAddress,
            }
        );
        assert!(queue.is_empty());

        server.join().unwrap();
    }
}
