use std::collections::HashMap;
use urt_core::{
    decode_exact, encode_exact, serialization_time, sha256, DecodeBudget,
    Digest32,
};

fn verify(name: &str, input: &[u8], base: Option<&[u8]>) {
    let encoded = encode_exact(input, base);
    let wire = encoded.packet.to_bytes();
    let mut cache = HashMap::<Digest32, Vec<u8>>::new();
    if let Some(base) = base {
        cache.insert(sha256(base), base.to_vec());
    }

    let decoded = decode_exact(
        &wire,
        &cache,
        DecodeBudget {
            max_output_bytes: input.len() as u64,
            max_decode_ops: (input.len() as u64)
                .saturating_mul(2)
                .saturating_add(1024),
            max_extra_working_bytes: input.len().max(8 * 1024),
        },
    )
    .expect("exact URT reconstruction");

    assert_eq!(decoded, input);
    let at_10_bps = serialization_time(encoded.report.network_bytes, 10)
        .expect("non-zero bitrate");

    println!(
        "URT_V0_CASE name={name} strategy={:?} original_bytes={} network_bytes={} shared_state_bytes={} ten_bps_seconds={:.3}",
        encoded.report.strategy,
        encoded.report.original_bytes,
        encoded.report.network_bytes,
        encoded.report.shared_state_bytes,
        at_10_bps.as_secs_f64(),
    );
}

fn main() {
    let repeated = b"ABCD".repeat(256 * 1024);
    verify("repeat-1mib", &repeated, None);

    let mut base = Vec::with_capacity(256 * 1024);
    for i in 0..256 * 1024 {
        base.push((i % 251) as u8);
    }
    let mut changed = base.clone();
    changed[120_000..120_032].copy_from_slice(&[0xA5; 32]);
    verify("delta-256kib", &changed, Some(&base));
    verify("cache-hit-256kib", &base, Some(&base));

    let mut state = 0x1234_5678_9abc_def0_u64;
    let mut noisy = vec![0_u8; 64 * 1024];
    for byte in &mut noisy {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
    verify("high-entropy-like-64kib", &noisy, None);

    valueless_structured_text_case();

    println!("URT_V1_PASS exact_cases=5");
}

fn valueless_structured_text_case() {
    let mut text = Vec::new();
    for index in 0..20_000_u32 {
        let line = format!(
            "{{\"id\":{index},\"kind\":\"weather\",\"city\":\"Hai Phong\",\"unit\":\"celsius\",\"valid\":true}}\n"
        );
        text.extend_from_slice(line.as_bytes());
    }
    verify("structured-text-zstandard", &text, None);
}
