use peer_egress::{decode_request, encode_request, ResolveRequest};
use signal_frontier::{
    apply_mechanical_channel, bit_error_count, decode_vibration_ook,
    encode_vibration_ook, MechanicalChannel, VibrationOokConfig,
};
use tiny_capsule::{Capsule, CapsuleError, CapsuleKey, ReplayGuard};

const VIBRATION_MAGIC: u32 = u32::from_be_bytes(*b"SP3V");
type VibrationCapsule = Capsule<VIBRATION_MAGIC>;
type VibrationReplayGuard = ReplayGuard<VIBRATION_MAGIC>;

fn main() {
    let request = ResolveRequest {
        request_id: 0x0D0E_0F10,
        relays_remaining: 1,
        hostname: "example.com".to_owned(),
    };
    let payload = encode_request(&request).expect("encode resolve request");

    let key = CapsuleKey::new([0x56; 32]);
    let capsule = VibrationCapsule {
        sender_id: 3003,
        sequence: 12,
        payload: payload.clone(),
    };
    let wire = capsule.seal(&key).expect("seal SP3V capsule");
    assert_eq!(&wire[..4], b"SP3V");

    let bits = bytes_to_bits(&wire);
    let config = VibrationOokConfig::surface_2_5bps();
    let clean =
        encode_vibration_ook(&bits, config).expect("vibration OOK encode");
    let impaired = apply_mechanical_channel(
        &clean,
        MechanicalChannel::shared_table(),
        0x56A0_0C12,
    )
    .expect("mechanical channel");
    let recovered_bits =
        decode_vibration_ook(&impaired, config).expect("vibration decode");

    assert_eq!(bit_error_count(&bits, &recovered_bits), 0);
    let recovered_wire = bits_to_bytes(&recovered_bits);
    assert_eq!(recovered_wire, wire);

    let mut replay = VibrationReplayGuard::new(32);
    let recovered = replay
        .open_and_accept(&recovered_wire, &key)
        .expect("authenticated SP3V capsule");
    let recovered_request =
        decode_request(&recovered.payload).expect("decode resolve request");
    assert_eq!(recovered_request, request);

    assert_eq!(
        replay.open_and_accept(&recovered_wire, &key).unwrap_err(),
        CapsuleError::ReplayDetected,
    );

    let modeled_duration_ms =
        bits.len() as u64 * u64::from(config.bit_duration_ms);
    let useful_payload_bps = if modeled_duration_ms == 0 {
        0.0
    } else {
        payload.len() as f64 * 8_000.0 / modeled_duration_ms as f64
    };

    println!(
        "F6_VIBRATION_CAPSULE_PASS hostname={} payload_bytes={} capsule_bytes={} bits={} samples={} modeled_duration_ms={} useful_payload_bps={:.3} replay_rejected=true",
        request.hostname,
        payload.len(),
        wire.len(),
        bits.len(),
        impaired.len(),
        modeled_duration_ms,
        useful_payload_bps,
    );
}

fn bytes_to_bits(bytes: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &byte in bytes {
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }
    bits
}

fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
    let (chunks, remainder) = bits.as_chunks::<8>();
    assert!(remainder.is_empty());
    chunks
        .iter()
        .map(|chunk| {
            chunk.iter().fold(0_u8, |value, bit| {
                (value << 1) | *bit
            })
        })
        .collect()
}
