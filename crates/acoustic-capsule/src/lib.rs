pub use tiny_capsule::{
    CapsuleError, HEADER_BYTES, MAX_PAYLOAD_BYTES, MIN_WIRE_BYTES, TAG_BYTES,
    VERSION,
};

pub const MAGIC_BYTES: [u8; 4] = *b"SP3A";
pub const MAGIC: u32 = u32::from_be_bytes(MAGIC_BYTES);

pub type AcousticCapsule = tiny_capsule::Capsule<MAGIC>;
pub type AcousticCapsuleKey = tiny_capsule::CapsuleKey;
pub type ReplayGuard = tiny_capsule::ReplayGuard<MAGIC>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acoustic_alias_preserves_sp3a_wire_domain() {
        let key = AcousticCapsuleKey::new([0xA6; 32]);
        let capsule = AcousticCapsule {
            sender_id: 42,
            sequence: 7,
            payload: b"example.com".to_vec(),
        };
        let wire = capsule.seal(&key).unwrap();

        assert_eq!(&wire[..4], &MAGIC_BYTES);
        assert_eq!(
            AcousticCapsule::open(&wire, &key).unwrap(),
            capsule,
        );
    }

    #[test]
    fn acoustic_alias_preserves_replay_guard_behavior() {
        let key = AcousticCapsuleKey::new([0xA6; 32]);
        let capsule = AcousticCapsule {
            sender_id: 7,
            sequence: 9,
            payload: b"x".to_vec(),
        };
        let wire = capsule.seal(&key).unwrap();
        let mut guard = ReplayGuard::new(4);

        guard.open_and_accept(&wire, &key).unwrap();
        assert_eq!(
            guard.open_and_accept(&wire, &key).unwrap_err(),
            CapsuleError::ReplayDetected,
        );
    }
}
