use sha2::{Digest, Sha256};
use signal_frontier::{
    bit_error_count, encode_optical_repetition, render_optical_cells,
    AcousticFskConfig, OpticalGrayFrame, OpticalGridConfig,
    OpticalRepetitionConfig, VibrationOokConfig,
};
use signal_trace_replay::{
    parse_pcm16_wav, parse_scalar_csv_column,
    parse_y4m_gray_video, register_optical_translation,
    register_optical_translation_scale, replay_acoustic_wav_window,
    replay_optical_pgm_sequence, replay_optical_pgm_sequence_registered,
    replay_optical_pgm_sequence_registered_scaled,
    replay_optical_y4m_registered_scaled, replay_vibration_csv_window,
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
        "optical-pgm" => optical_pgm(&args[2..]),
        "optical-pgm-auto" => optical_pgm_auto(&args[2..]),
        "optical-pgm-auto-scale" => optical_pgm_auto_scale(&args[2..]),
        "optical-y4m-auto-scale" => optical_y4m_auto_scale(&args[2..]),
        "optical-y4m-fixture" => optical_y4m_fixture(&args[2..]),
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

fn optical_pgm(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() < 2 {
        return Err(usage().into());
    }

    let expected = hex_to_bits(&args[0])?;
    let repetition = OpticalRepetitionConfig::robust_default();
    let grid = OpticalGridConfig::camera_baseline();
    let symbol_count = expected
        .len()
        .checked_mul(repetition.repeats_per_bit)
        .ok_or("optical symbol count overflow")?;

    let mut frames = Vec::with_capacity(args.len() - 1);
    let mut hashes = Vec::with_capacity(args.len() - 1);
    for path in &args[1..] {
        let bytes = fs::read(path)?;
        hashes.push(sha256_hex(&bytes));
        frames.push(bytes);
    }

    let decoded = replay_optical_pgm_sequence(
        &frames,
        symbol_count,
        grid,
        None,
        repetition,
    )?;
    let errors = bit_error_count(&expected, &decoded);

    println!(
        "F3_OPTICAL_REPLAY evidence_level=UNCLASSIFIED_REPLAY frames={} frame_sha256={} known_geometry=true expected_bits={} decoded_bits={} bit_errors={}",
        frames.len(),
        hashes.join(","),
        expected.len(),
        decoded.len(),
        errors,
    );

    if errors != 0 {
        return Err(format!("optical replay has {errors} bit errors").into());
    }
    Ok(())
}

fn optical_pgm_auto(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() < 2 {
        return Err(usage().into());
    }

    let expected = hex_to_bits(&args[0])?;
    let repetition = OpticalRepetitionConfig::robust_default();
    let grid = OpticalGridConfig::camera_baseline();
    let symbol_count = expected
        .len()
        .checked_mul(repetition.repeats_per_bit)
        .ok_or("optical symbol count overflow")?;

    let mut frames = Vec::with_capacity(args.len() - 1);
    let mut hashes = Vec::with_capacity(args.len() - 1);
    let mut origins = Vec::with_capacity(args.len() - 1);

    for path in &args[1..] {
        let bytes = fs::read(path)?;
        let pgm = signal_trace_replay::parse_pgm_gray8(&bytes)?;
        let source = pgm.to_optical_frame();
        let (_, registration) =
            register_optical_translation(&source, symbol_count, grid)?;
        hashes.push(sha256_hex(&bytes));
        origins.push(format!(
            "{}:{}",
            registration.source_origin_x,
            registration.source_origin_y,
        ));
        frames.push(bytes);
    }

    let decoded = replay_optical_pgm_sequence_registered(
        &frames,
        symbol_count,
        grid,
        repetition,
    )?;
    let errors = bit_error_count(&expected, &decoded);

    println!(
        "F3_OPTICAL_AUTO_REPLAY evidence_level=UNCLASSIFIED_REPLAY frames={} frame_sha256={} registration=translation origins={} expected_bits={} decoded_bits={} bit_errors={}",
        frames.len(),
        hashes.join(","),
        origins.join(","),
        expected.len(),
        decoded.len(),
        errors,
    );

    if errors != 0 {
        return Err(format!("optical auto replay has {errors} bit errors").into());
    }
    Ok(())
}

fn optical_pgm_auto_scale(
    args: &[String],
) -> Result<(), Box<dyn Error>> {
    if args.len() < 2 {
        return Err(usage().into());
    }

    let expected = hex_to_bits(&args[0])?;
    let repetition = OpticalRepetitionConfig::robust_default();
    let grid = OpticalGridConfig::camera_baseline();
    let symbol_count = expected
        .len()
        .checked_mul(repetition.repeats_per_bit)
        .ok_or("optical symbol count overflow")?;

    let mut frames = Vec::with_capacity(args.len() - 1);
    let mut hashes = Vec::with_capacity(args.len() - 1);
    let mut registrations = Vec::with_capacity(args.len() - 1);

    for path in &args[1..] {
        let bytes = fs::read(path)?;
        let pgm = signal_trace_replay::parse_pgm_gray8(&bytes)?;
        let source = pgm.to_optical_frame();
        let (_, registration) =
            register_optical_translation_scale(
                &source,
                symbol_count,
                grid,
            )?;
        hashes.push(sha256_hex(&bytes));
        registrations.push(format!(
            "{}:{}:{:.4}:{:.4}",
            registration.source_origin_x,
            registration.source_origin_y,
            registration.scale_x,
            registration.scale_y,
        ));
        frames.push(bytes);
    }

    let decoded = replay_optical_pgm_sequence_registered_scaled(
        &frames,
        symbol_count,
        grid,
        repetition,
    )?;
    let errors = bit_error_count(&expected, &decoded);

    println!(
        "F3_OPTICAL_AUTO_SCALE_REPLAY evidence_level=UNCLASSIFIED_REPLAY frames={} frame_sha256={} registration=translation_uniform_scale values={} expected_bits={} decoded_bits={} bit_errors={}",
        frames.len(),
        hashes.join(","),
        registrations.join(","),
        expected.len(),
        decoded.len(),
        errors,
    );

    if errors != 0 {
        return Err(
            format!("optical auto-scale replay has {errors} bit errors")
                .into(),
        );
    }
    Ok(())
}

fn optical_y4m_auto_scale(
    args: &[String],
) -> Result<(), Box<dyn Error>> {
    if !(2..=4).contains(&args.len()) {
        return Err(usage().into());
    }

    let expected = hex_to_bits(&args[0])?;
    let path = &args[1];
    let start_frame =
        parse_optional_usize(args.get(2), 0, "start_frame")?;
    let bytes = fs::read(path)?;
    let video = parse_y4m_gray_video(&bytes)?;
    let available = video
        .luma_frames
        .len()
        .checked_sub(start_frame)
        .ok_or("start_frame is past end of video")?;
    let frame_count =
        parse_optional_usize(args.get(3), available, "frame_count")?;
    if frame_count == 0 {
        return Err("frame_count must be positive".into());
    }

    let repetition = OpticalRepetitionConfig::robust_default();
    let grid = OpticalGridConfig::camera_baseline();
    let symbol_count = expected
        .len()
        .checked_mul(repetition.repeats_per_bit)
        .ok_or("optical symbol count overflow")?;

    let decoded = replay_optical_y4m_registered_scaled(
        &bytes,
        symbol_count,
        grid,
        repetition,
        start_frame,
        frame_count,
    )?;
    let errors = bit_error_count(&expected, &decoded);

    println!(
        "F3_OPTICAL_Y4M_REPLAY input={} evidence_level=UNCLASSIFIED_REPLAY sha256={} width={} height={} fps={}:{} chroma={} total_frames={} start_frame={} frame_count={} registration=translation_uniform_scale expected_bits={} decoded_bits={} bit_errors={}",
        path,
        sha256_hex(&bytes),
        video.width,
        video.height,
        video.fps_numerator,
        video.fps_denominator,
        video.chroma,
        video.luma_frames.len(),
        start_frame,
        frame_count,
        expected.len(),
        decoded.len(),
        errors,
    );

    if errors != 0 {
        return Err(format!("optical Y4M replay has {errors} bit errors").into());
    }
    Ok(())
}

fn optical_y4m_fixture(
    args: &[String],
) -> Result<(), Box<dyn Error>> {
    if !(2..=5).contains(&args.len()) {
        return Err(usage().into());
    }

    let bits = hex_to_bits(&args[0])?;
    let output = &args[1];
    let canvas_width =
        parse_optional_usize(args.get(2), 640, "width")?;
    let canvas_height =
        parse_optional_usize(args.get(3), 480, "height")?;
    let frame_count =
        parse_optional_usize(args.get(4), 90, "frame_count")?;
    if canvas_width == 0 || canvas_height == 0 || frame_count == 0 {
        return Err("fixture dimensions/frame_count must be positive".into());
    }

    let repetition = OpticalRepetitionConfig::robust_default();
    let grid = OpticalGridConfig::camera_baseline();
    let symbols = encode_optical_repetition(&bits, repetition)
        .map_err(|error| format!("optical repetition encode failed: {error:?}"))?;
    let base = render_optical_cells(&symbols, grid)
        .map_err(|error| format!("optical raster render failed: {error:?}"))?;
    let scaled = resize_optical_frame_nearest(&base, 1, 1)?;
    if scaled.width > canvas_width || scaled.height > canvas_height {
        return Err(format!(
            "scaled optical raster {}x{} does not fit canvas {}x{}",
            scaled.width,
            scaled.height,
            canvas_width,
            canvas_height,
        )
        .into());
    }

    let offset_x = (canvas_width - scaled.width) / 2;
    let offset_y = (canvas_height - scaled.height) / 2;
    let canvas = embed_optical_frame_centered(
        &scaled,
        canvas_width,
        canvas_height,
        offset_x,
        offset_y,
    )?;

    let mut y4m = format!(
        "YUV4MPEG2 W{} H{} F30:1 Ip Cmono\n",
        canvas_width,
        canvas_height,
    )
    .into_bytes();

    let luma = canvas
        .pixels
        .iter()
        .map(|value| {
            (value.clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect::<Vec<_>>();

    for _ in 0..frame_count {
        y4m.extend_from_slice(b"FRAME\n");
        y4m.extend_from_slice(&luma);
    }

    fs::write(output, &y4m)?;
    println!(
        "F5_CAMERA_FIXTURE_READY output={} sha256={} width={} height={} frames={} payload_bits={} symbols={} raster={}x{} offset={}:{}",
        output,
        sha256_hex(&y4m),
        canvas_width,
        canvas_height,
        frame_count,
        bits.len(),
        symbols.len(),
        scaled.width,
        scaled.height,
        offset_x,
        offset_y,
    );
    Ok(())
}

fn resize_optical_frame_nearest(
    frame: &OpticalGrayFrame,
    numerator: usize,
    denominator: usize,
) -> Result<OpticalGrayFrame, Box<dyn Error>> {
    if numerator == 0 || denominator == 0 {
        return Err("optical resize ratio must be positive".into());
    }
    let width = frame
        .width
        .checked_mul(numerator)
        .ok_or("optical resize width overflow")?
        / denominator;
    let height = frame
        .height
        .checked_mul(numerator)
        .ok_or("optical resize height overflow")?
        / denominator;
    if width == 0 || height == 0 {
        return Err("optical resize produced zero dimension".into());
    }

    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let source_y = (y * denominator / numerator)
            .min(frame.height - 1);
        for x in 0..width {
            let source_x = (x * denominator / numerator)
                .min(frame.width - 1);
            pixels.push(
                frame.pixels[source_y * frame.width + source_x],
            );
        }
    }

    Ok(OpticalGrayFrame {
        width,
        height,
        pixels,
    })
}

fn embed_optical_frame_centered(
    frame: &OpticalGrayFrame,
    canvas_width: usize,
    canvas_height: usize,
    offset_x: usize,
    offset_y: usize,
) -> Result<OpticalGrayFrame, Box<dyn Error>> {
    if offset_x + frame.width > canvas_width
        || offset_y + frame.height > canvas_height
    {
        return Err("optical raster exceeds fixture canvas".into());
    }

    let mut canvas = OpticalGrayFrame {
        width: canvas_width,
        height: canvas_height,
        pixels: vec![0.02; canvas_width * canvas_height],
    };

    for y in 0..frame.height {
        let src = y * frame.width;
        let dst = (offset_y + y) * canvas_width + offset_x;
        canvas.pixels[dst..dst + frame.width]
            .copy_from_slice(&frame.pixels[src..src + frame.width]);
    }

    Ok(canvas)
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
        "  signal-trace-replay-cli optical-pgm <expected_hex> <frame1.pgm> [frame2.pgm ...]",
        "  signal-trace-replay-cli optical-pgm-auto <expected_hex> <frame1.pgm> [frame2.pgm ...]",
        "  signal-trace-replay-cli optical-pgm-auto-scale <expected_hex> <frame1.pgm> [frame2.pgm ...]",
        "  signal-trace-replay-cli optical-y4m-auto-scale <expected_hex> <capture.y4m> [start_frame] [frame_count]",
        "  signal-trace-replay-cli optical-y4m-fixture <expected_hex> <output.y4m> [width] [height] [frame_count]",
        "",
        "All commands use the current F3 reference decoder profiles.",
        "Input provenance is always printed as UNCLASSIFIED_REPLAY.",
        "optical-pgm requires crop/resize to the known court geometry.",
        "optical-pgm-auto may discover translation inside a larger same-scale grayscale canvas.",
        "optical-pgm-auto-scale additionally estimates bounded uniform scale and resamples to the reference grid.",
        "optical-y4m-auto-scale extracts luma frames from Y4M video and applies the same registration/replay path.",
        "optical-y4m-fixture emits a deterministic 2x optical raster on a mono Y4M canvas for emulator-camera courts.",
        "A physical court must separately prove how each capture was produced.",
    ]
    .join("\n")
}
