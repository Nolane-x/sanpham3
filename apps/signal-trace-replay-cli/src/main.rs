use sha2::{Digest, Sha256};
use signal_frontier::{
    bit_error_count, AcousticFskConfig, VibrationOokConfig,
};
use signal_trace_replay::{
    parse_pcm16_wav, parse_scalar_csv_column,
    replay_acoustic_wav_window, replay_vibration_csv_window,
};
use std::env;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 2 {
        return Err(usage().into());
    }

    match args[1].as_str() {
        "acoustic-wav" => acoustic_wav(&args[2..]),
        "vibration-csv" => vibration_csv(&args[2..]),
        _ => Err(usage().into()),
    }
}

fn acoustic_wav(args: &[String]) -> Result<(), Box<dyn Error>> {
    if !(2..=4).contains(&args.len()) {
        return Err(usage().into());
    }

    let path = &args[0];
    let expected = hex_to_bits(&args[1])?;
    let channel = parse_optional_usize(args.get(2), 0, "channel")?;
    let start_sample =
        parse_optional_usize(args.get(3), 0, "start_sample")?;
    let bytes = fs::read(path)?;
    let wav = parse_pcm16_wav(&bytes)?;
    let config = AcousticFskConfig::near_ultrasonic_50bps();
    let decoded = replay_acoustic_wav_window(
        &bytes,
        channel,
        config,
        start_sample,
        expected.len(),
    )?;
    let errors = bit_error_count(&expected, &decoded.bits);

    println!(
        "F3_ACOUSTIC_REPLAY input={} evidence_level=UNCLASSIFIED_REPLAY sha256={} sample_rate_hz={} channels={} channel={} start_sample={} expected_bits={} decoded_bits={} bit_errors={} min_confidence={:.4}",
        path,
        sha256_hex(&bytes),
        wav.sample_rate_hz,
        wav.channels,
        channel,
        start_sample,
        expected.len(),
        decoded.bits.len(),
        errors,
        decoded.minimum_confidence,
    );

    if errors != 0 {
        return Err(format!("acoustic replay has {errors} bit errors").into());
    }
    Ok(())
}

fn vibration_csv(args: &[String]) -> Result<(), Box<dyn Error>> {
    if !(2..=4).contains(&args.len()) {
        return Err(usage().into());
    }

    let path = &args[0];
    let expected = hex_to_bits(&args[1])?;
    let value_column =
        parse_optional_usize(args.get(2), 1, "value_column")?;
    let start_sample =
        parse_optional_usize(args.get(3), 0, "start_sample")?;
    let bytes = fs::read(path)?;
    let text = std::str::from_utf8(&bytes)?;
    let parsed = parse_scalar_csv_column(text, value_column)?;
    let config = VibrationOokConfig::surface_2_5bps();
    let decoded = replay_vibration_csv_window(
        text,
        value_column,
        config,
        start_sample,
        expected.len(),
    )?;
    let errors = bit_error_count(&expected, &decoded);

    println!(
        "F3_VIBRATION_REPLAY input={} evidence_level=UNCLASSIFIED_REPLAY sha256={} parsed_samples={} value_column={} start_sample={} expected_bits={} decoded_bits={} bit_errors={}",
        path,
        sha256_hex(&bytes),
        parsed.values.len(),
        value_column,
        start_sample,
        expected.len(),
        decoded.len(),
        errors,
    );

    if errors != 0 {
        return Err(format!("vibration replay has {errors} bit errors").into());
    }
    Ok(())
}

fn parse_optional_usize(
    value: Option<&String>,
    default: usize,
    name: &str,
) -> Result<usize, Box<dyn Error>> {
    match value {
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| format!("invalid {name}: {raw}").into()),
        None => Ok(default),
    }
}

fn hex_to_bits(raw: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let compact = raw.strip_prefix("0x").unwrap_or(raw);
    if compact.is_empty() || !compact.len().is_multiple_of(2) {
        return Err("expected_hex must contain a non-empty even number of hex digits".into());
    }

    let mut bits = Vec::with_capacity(compact.len() * 4);
    for pair in compact.as_bytes().as_chunks::<2>().0 {
        let text = std::str::from_utf8(pair)?;
        let byte = u8::from_str_radix(text, 16)?;
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }
    Ok(bits)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn usage() -> String {
    [
        "usage:",
        "  signal-trace-replay-cli acoustic-wav <capture.wav> <expected_hex> [channel] [start_sample]",
        "  signal-trace-replay-cli vibration-csv <capture.csv> <expected_hex> [value_column] [start_sample]",
        "",
        "Both commands use the current F3 reference decoder profiles.",
        "Input provenance is always printed as UNCLASSIFIED_REPLAY.",
        "A physical court must separately prove how the capture was produced.",
    ]
    .join("\n")
}
