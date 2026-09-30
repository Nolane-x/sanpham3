use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::fmt;

type HmacSha256 = Hmac<Sha256>;

pub const VERSION: u8 = 1;
pub const TAG_BYTES: usize = 16;
pub const HEADER_BYTES: usize = 4 + 1 + 8 + 8 + 2;
pub const MIN_WIRE_BYTES: usize = HEADER_BYTES + TAG_BYTES;
pub const MAX_PAYLOAD_BYTES: usize = 96;

pub struct CapsuleKey([u8; 32]);

impl CapsuleKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for CapsuleKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capsule<const MAGIC: u32> {
    pub sender_id: u64,
    pub sequence: u64,
    pub payload: Vec<u8>,
}

impl<const MAGIC: u32> Capsule<MAGIC> {
    pub fn magic_bytes() -> [u8; 4] {
        MAGIC.to_be_bytes()
    }

    pub fn seal(&self, key: &CapsuleKey) -> Result<Vec<u8>, CapsuleError> {
        validate_payload(&self.payload)?;
        let payload_len = u16::try_from(self.payload.len())
            .map_err(|_| CapsuleError::PayloadTooLarge)?;

        let mut wire = Vec::with_capacity(
            HEADER_BYTES + self.payload.len() + TAG_BYTES,
        );
        wire.extend_from_slice(&Self::magic_bytes());
        wire.push(VERSION);
        wire.extend_from_slice(&self.sender_id.to_be_bytes());
        wire.extend_from_slice(&self.sequence.to_be_bytes());
        wire.extend_from_slice(&payload_len.to_be_bytes());
        wire.extend_from_slice(&self.payload);

        let tag = authentication_tag(&wire, key);
        wire.extend_from_slice(&tag);
        Ok(wire)
    }

    pub fn open(
        wire: &[u8],
        key: &CapsuleKey,
    ) -> Result<Self, CapsuleError> {
        if wire.len() < MIN_WIRE_BYTES {
            return Err(CapsuleError::Truncated);
        }

        let authenticated_len = wire.len() - TAG_BYTES;
        let provided_tag = &wire[authenticated_len..];
        let expected_tag =
            authentication_tag(&wire[..authenticated_len], key);
        if !constant_time_eq(provided_tag, &expected_tag) {
            return Err(CapsuleError::AuthenticationFailed);
        }

        let mut cursor = Cursor::new(&wire[..authenticated_len]);
        if cursor.take(4)? != Self::magic_bytes() {
            return Err(CapsuleError::WrongMagic);
        }
        let version = cursor.u8()?;
        if version != VERSION {
            return Err(CapsuleError::WrongVersion(version));
        }

        let sender_id = cursor.u64()?;
        let sequence = cursor.u64()?;
        let payload_len = cursor.u16()? as usize;
        let payload = cursor.take(payload_len)?.to_vec();

        if cursor.remaining() != 0 {
            return Err(CapsuleError::TrailingBytes);
        }
        validate_payload(&payload)?;

        Ok(Self {
            sender_id,
            sequence,
            payload,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReplayWindow {
    highest: u64,
    bitmap: u64,
}

#[derive(Debug)]
pub struct ReplayGuard<const MAGIC: u32> {
    max_senders: usize,
    windows: HashMap<u64, ReplayWindow>,
}

impl<const MAGIC: u32> ReplayGuard<MAGIC> {
    pub fn new(max_senders: usize) -> Self {
        Self {
            max_senders,
            windows: HashMap::new(),
        }
    }

    pub fn accept(
        &mut self,
        sender_id: u64,
        sequence: u64,
    ) -> Result<(), CapsuleError> {
        if let Some(window) = self.windows.get_mut(&sender_id) {
            if sequence > window.highest {
                let advance = sequence - window.highest;
                window.bitmap = if advance >= 64 {
                    1
                } else {
                    (window.bitmap << advance) | 1
                };
                window.highest = sequence;
                return Ok(());
            }

            let distance = window.highest - sequence;
            if distance >= 64 {
                return Err(CapsuleError::ReplayTooOld);
            }

            let mask = 1_u64 << distance;
            if window.bitmap & mask != 0 {
                return Err(CapsuleError::ReplayDetected);
            }
            window.bitmap |= mask;
            return Ok(());
        }

        if self.max_senders == 0 || self.windows.len() >= self.max_senders {
            return Err(CapsuleError::ReplayCapacity);
        }

        self.windows.insert(
            sender_id,
            ReplayWindow {
                highest: sequence,
                bitmap: 1,
            },
        );
        Ok(())
    }

    pub fn open_and_accept(
        &mut self,
        wire: &[u8],
        key: &CapsuleKey,
    ) -> Result<Capsule<MAGIC>, CapsuleError> {
        let capsule = Capsule::<MAGIC>::open(wire, key)?;
        self.accept(capsule.sender_id, capsule.sequence)?;
        Ok(capsule)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsuleError {
    Truncated,
    WrongMagic,
    WrongVersion(u8),
    AuthenticationFailed,
    PayloadEmpty,
    PayloadTooLarge,
    TrailingBytes,
    ReplayDetected,
    ReplayTooOld,
    ReplayCapacity,
}

impl fmt::Display for CapsuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "tiny capsule is truncated"),
            Self::WrongMagic => write!(f, "tiny capsule has wrong domain magic"),
            Self::WrongVersion(version) => {
                write!(f, "unsupported tiny capsule version {version}")
            }
            Self::AuthenticationFailed => {
                write!(f, "tiny capsule authentication failed")
            }
            Self::PayloadEmpty => write!(f, "tiny capsule payload is empty"),
            Self::PayloadTooLarge => write!(f, "tiny capsule payload is too large"),
            Self::TrailingBytes => write!(f, "tiny capsule contains trailing bytes"),
            Self::ReplayDetected => write!(f, "tiny capsule replay detected"),
            Self::ReplayTooOld => write!(f, "tiny capsule sequence is outside replay window"),
            Self::ReplayCapacity => write!(f, "tiny capsule replay guard sender capacity exhausted"),
        }
    }
}

impl std::error::Error for CapsuleError {}

fn validate_payload(payload: &[u8]) -> Result<(), CapsuleError> {
    if payload.is_empty() {
        return Err(CapsuleError::PayloadEmpty);
    }
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(CapsuleError::PayloadTooLarge);
    }
    Ok(())
}

fn authentication_tag(
    bytes: &[u8],
    key: &CapsuleKey,
) -> [u8; TAG_BYTES] {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(bytes);
    let digest = mac.finalize().into_bytes();
    let mut tag = [0_u8; TAG_BYTES];
    tag.copy_from_slice(&digest[..TAG_BYTES]);
    tag
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (&a, &b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CapsuleError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(CapsuleError::Truncated)?;
        if end > self.bytes.len() {
            return Err(CapsuleError::Truncated);
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, CapsuleError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, CapsuleError> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| CapsuleError::Truncated)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, CapsuleError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| CapsuleError::Truncated)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MAGIC: u32 = u32::from_be_bytes(*b"SP3T");

    type TestCapsule = Capsule<TEST_MAGIC>;
    type TestReplayGuard = ReplayGuard<TEST_MAGIC>;

    fn key() -> CapsuleKey {
        CapsuleKey::new([0x5A; 32])
    }

    #[test]
    fn domain_separated_capsule_roundtrips() {
        let capsule = TestCapsule {
            sender_id: 42,
            sequence: 7,
            payload: b"example.com".to_vec(),
        };
        let wire = capsule.seal(&key()).unwrap();

        assert_eq!(&wire[..4], b"SP3T");
        assert_eq!(TestCapsule::open(&wire, &key()).unwrap(), capsule);
    }

    #[test]
    fn different_domain_rejects_same_authenticated_wire() {
        const OTHER_MAGIC: u32 = u32::from_be_bytes(*b"SP3X");
        type OtherCapsule = Capsule<OTHER_MAGIC>;

        let capsule = TestCapsule {
            sender_id: 1,
            sequence: 1,
            payload: b"x".to_vec(),
        };
        let wire = capsule.seal(&key()).unwrap();

        assert_eq!(
            OtherCapsule::open(&wire, &key()).unwrap_err(),
            CapsuleError::WrongMagic,
        );
    }

    #[test]
    fn tampering_and_wrong_key_fail_authentication() {
        let capsule = TestCapsule {
            sender_id: 42,
            sequence: 7,
            payload: b"example.com".to_vec(),
        };
        let mut wire = capsule.seal(&key()).unwrap();
        wire[HEADER_BYTES] ^= 1;
        assert_eq!(
            TestCapsule::open(&wire, &key()).unwrap_err(),
            CapsuleError::AuthenticationFailed,
        );

        let clean = capsule.seal(&key()).unwrap();
        let wrong = CapsuleKey::new([0x99; 32]);
        assert_eq!(
            TestCapsule::open(&clean, &wrong).unwrap_err(),
            CapsuleError::AuthenticationFailed,
        );
    }

    #[test]
    fn replay_window_accepts_small_reordering_once() {
        let mut guard = TestReplayGuard::new(4);
        assert_eq!(guard.accept(1, 10), Ok(()));
        assert_eq!(guard.accept(1, 12), Ok(()));
        assert_eq!(guard.accept(1, 11), Ok(()));
        assert_eq!(guard.accept(1, 11), Err(CapsuleError::ReplayDetected));
    }

    #[test]
    fn replay_window_rejects_too_old_sequences() {
        let mut guard = TestReplayGuard::new(4);
        guard.accept(1, 100).unwrap();
        assert_eq!(
            guard.accept(1, 35),
            Err(CapsuleError::ReplayTooOld),
        );
    }

    #[test]
    fn replay_guard_bounds_sender_state() {
        let mut guard = TestReplayGuard::new(1);
        guard.accept(1, 1).unwrap();
        assert_eq!(
            guard.accept(2, 1),
            Err(CapsuleError::ReplayCapacity),
        );
    }

    #[test]
    fn payload_budget_is_enforced() {
        let too_large = TestCapsule {
            sender_id: 1,
            sequence: 1,
            payload: vec![0; MAX_PAYLOAD_BYTES + 1],
        };
        assert_eq!(
            too_large.seal(&key()).unwrap_err(),
            CapsuleError::PayloadTooLarge,
        );
    }
}
