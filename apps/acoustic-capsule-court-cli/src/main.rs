use acoustic_capsule::{
    AcousticCapsule, AcousticCapsuleKey, CapsuleError, ReplayGuard,
};
use peer_egress::{decode_request, encode_request, ResolveRequest};
use signal_frontier::{
    apply_acoustic_channel, apply_acoustic_impulse_response,
    apply_clock_drift_resampling, bit_error_count, decode_fsk, encode_fsk,
    AcousticChannel, AcousticFskConfig, AcousticImpulseTap,
};

fn main() {
    let request = ResolveRequest {
        request_id: 0x1020_3040,
        relays_remaining: 1,
        hostname: "example.com".to_owned(),
    };
    let payload = encode_request(&request).expect("encode resolve request");

    let key = AcousticCapsuleKey::new([0xA6; 32]);
    let capsule = AcousticCapsule {
        sender_id: 1001,
        sequence: 77,
        payload: payload.clone(),
    };
    let wire = capsule.seal(&key).expect("seal acoustic capsule");
    let bits = bytes_to_bits(&wire);

    let config = AcousticFskConfig::near_ultrasonic_50bps();
    let clean = encode_fsk(&bits, config).expect("FSK encode");
    let multipath = apply_acoustic_impulse_response(
        &clean,
        &[
            AcousticImpulseTap {
                delay_samples: 0,
                gain: 0.74,
            },
            AcousticImpulseTap {
                delay_samples: 9,
                gain: 0.14,
            },
            AcousticImpulseTap {
                delay_samples: 23,
                gain: -0.05,
            },
        ],
    )
    .expect("multipath impairment");
    let drifted =
        apply_clock_drift_resampling(&multipath, 60).expect("clock drift");
    let impaired = apply_acoustic_channel(
        &drifted,
        AcousticChannel {
            gain: 0.92,
            white_noise_amplitude: 0.018,
            clip_level: 0.95,
        },
        0xA6C0_57C1,
    )
    .expect("acoustic channel");

    let decoded = decode_fsk(&impaired, config).expect("FSK decode");
    let errors = bit_error_count(&bits, &decoded.bits);
    assert_eq!(errors, 0, "capsule bitstream must survive synthetic court");

    let recovered_wire = bits_to_bytes(&decoded.bits);
    assert_eq!(recovered_wire, wire);

    let mut replay = ReplayGuard::new(32);
    let recovered = replay
        .open_and_accept(&recovered_wire, &key)
        .expect("authenticated capsule");
    assert_eq!(recovered.sender_id, capsule.sender_id);
    assert_eq!(recovered.sequence, capsule.sequence);

    let recovered_request =
        decode_request(&recovered.payload).expect("decode resolve request");
    assert_eq!(recovered_request, request);

    assert_eq!(
        replay.open_and_accept(&recovered_wire, &key).unwrap_err(),
        CapsuleError::ReplayDetected,
    );

    let modeled_duration_ms =
        (bits.len() as u64 * 1_000).div_ceil(config.bit_rate_bps as u64);
    let useful_payload_bps = if modeled_duration_ms == 0 {
        0.0
    } else {
        payload.len() as f64 * 8_000.0 / modeled_duration_ms as f64
    };

    println!(
        "F6_ACOUSTIC_CAPSULE_PASS hostname={} payload_bytes={} capsule_bytes={} bits={} modeled_duration_ms={} useful_payload_bps={:.2} min_confidence={:.4} replay_rejected=true",
        request.hostname,
        payload.len(),
        wire.len(),
        bits.len(),
        modeled_duration_ms,
        useful_payload_bps,
        decoded.minimum_confidence,
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
    assert!(bits.len().is_multiple_of(8));
    bits.chunks_exact(8)
        .map(|chunk| {
            chunk.iter().fold(0_u8, |value, bit| {
                (value << 1) | *bit
            })
        })
        .collect()
}
