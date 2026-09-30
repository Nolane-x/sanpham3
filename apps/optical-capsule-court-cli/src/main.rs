use peer_egress::{decode_request, encode_request, ResolveRequest};
use signal_frontier::{
    bit_error_count, decode_optical_cells, decode_optical_repetition,
    encode_optical_repetition, render_optical_cells,
    apply_optical_box_blur, apply_optical_photometric,
    warp_optical_perspective, OpticalGridConfig, OpticalPerspective,
    OpticalPhotometric, OpticalRepetitionConfig,
};
use tiny_capsule::{Capsule, CapsuleError, CapsuleKey, ReplayGuard};

const OPTICAL_MAGIC: u32 = u32::from_be_bytes(*b"SP3O");
type OpticalCapsule = Capsule<OPTICAL_MAGIC>;
type OpticalReplayGuard = ReplayGuard<OPTICAL_MAGIC>;

fn main() {
    let request = ResolveRequest {
        request_id: 0x0A0B_0C0D,
        relays_remaining: 1,
        hostname: "example.com".to_owned(),
    };
    let payload = encode_request(&request).expect("encode resolve request");

    let key = CapsuleKey::new([0x0F; 32]);
    let capsule = OpticalCapsule {
        sender_id: 2002,
        sequence: 41,
        payload: payload.clone(),
    };
    let wire = capsule.seal(&key).expect("seal SP3O capsule");
    assert_eq!(&wire[..4], b"SP3O");

    let bits = bytes_to_bits(&wire);
    let repetition = OpticalRepetitionConfig::robust_default();
    let symbols =
        encode_optical_repetition(&bits, repetition).expect("repetition encode");

    let grid = OpticalGridConfig::camera_baseline();
    let perspective = OpticalPerspective::mild_keystone();
    let rendered =
        render_optical_cells(&symbols, grid).expect("render optical cells");
    let warped = warp_optical_perspective(&rendered, perspective)
        .expect("perspective warp");
    let blurred =
        apply_optical_box_blur(&warped, 1).expect("optical blur");
    let photographed = apply_optical_photometric(
        &blurred,
        OpticalPhotometric::phone_camera_baseline(),
    )
    .expect("optical photometric impairment");

    let sampled = decode_optical_cells(
        &photographed,
        symbols.len(),
        grid,
        Some(perspective),
    )
    .expect("sample optical cells");
    let recovered_bits =
        decode_optical_repetition(&sampled, repetition)
            .expect("repetition decode");
    assert_eq!(bit_error_count(&bits, &recovered_bits), 0);

    let recovered_wire = bits_to_bytes(&recovered_bits);
    assert_eq!(recovered_wire, wire);

    let mut replay = OpticalReplayGuard::new(32);
    let recovered = replay
        .open_and_accept(&recovered_wire, &key)
        .expect("authenticated SP3O capsule");
    let recovered_request =
        decode_request(&recovered.payload).expect("decode resolve request");
    assert_eq!(recovered_request, request);

    assert_eq!(
        replay.open_and_accept(&recovered_wire, &key).unwrap_err(),
        CapsuleError::ReplayDetected,
    );

    let erasures = sampled.iter().filter(|symbol| symbol.is_none()).count();
    println!(
        "F6_OPTICAL_CAPSULE_PASS hostname={} payload_bytes={} capsule_bytes={} bits={} symbols={} frame={}x{} erasures={} replay_rejected=true",
        request.hostname,
        payload.len(),
        wire.len(),
        bits.len(),
        symbols.len(),
        photographed.width,
        photographed.height,
        erasures,
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
    bits.as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| {
            chunk.iter().fold(0_u8, |value, bit| {
                (value << 1) | *bit
            })
        })
        .collect()
}
