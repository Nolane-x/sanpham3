use signal_frontier::{
    decode_fsk, decode_optical_cells, decode_optical_repetition,
    decode_vibration_ook, AcousticFskConfig, DecodeResult,
    OpticalGrayFrame, OpticalGridConfig, OpticalPerspective,
    OpticalRepetitionConfig, SignalError, VibrationOokConfig,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm16Wav {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub interleaved_samples: Vec<i16>,
}

impl Pcm16Wav {
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.interleaved_samples.len() / self.channels as usize
    }

    pub fn channel_f32(
        &self,
        channel_index: usize,
    ) -> Result<Vec<f32>, ReplayError> {
        if channel_index >= self.channels as usize {
            return Err(ReplayError::InvalidChannel {
                requested: channel_index,
                channels: self.channels,
            });
        }

        Ok(self
            .interleaved_samples
            .chunks_exact(self.channels as usize)
            .map(|frame| frame[channel_index] as f32 / 32768.0)
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScalarCsvTrace {
    pub values: Vec<f32>,
    pub skipped_header: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgmGray8 {
    pub width: usize,
    pub height: usize,
    pub max_value: u8,
    pub pixels: Vec<u8>,
}

impl PgmGray8 {
    pub fn to_optical_frame(&self) -> OpticalGrayFrame {
        let scale = 1.0_f32 / self.max_value as f32;
        OpticalGrayFrame {
            width: self.width,
            height: self.height,
            pixels: self
                .pixels
                .iter()
                .map(|&pixel| pixel as f32 * scale)
                .collect(),
        }
    }
}

#[derive(Debug)]
pub enum ReplayError {
    InvalidWav(&'static str),
    UnsupportedWav {
        format: u16,
        channels: u16,
        bits_per_sample: u16,
    },
    InvalidChannel {
        requested: usize,
        channels: u16,
    },
    InvalidCsvLine {
        line: usize,
    },
    InvalidCsvColumn {
        line: usize,
        requested: usize,
        columns: usize,
    },
    InvalidPgm(&'static str),
    UnsupportedPgmMaxValue(u32),
    OpticalVoteShapeMismatch,
    EmptyTrace,
    WindowOutsideTrace,
    SampleRateMismatch {
        expected: u32,
        actual: u32,
    },
    Signal(SignalError),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWav(message) => write!(f, "invalid WAV: {message}"),
            Self::UnsupportedWav {
                format,
                channels,
                bits_per_sample,
            } => write!(
                f,
                "unsupported WAV format={format} channels={channels} bits={bits_per_sample}",
            ),
            Self::InvalidChannel {
                requested,
                channels,
            } => write!(
                f,
                "WAV channel {requested} is outside {channels} channels",
            ),
            Self::InvalidCsvLine { line } => {
                write!(f, "invalid numeric CSV row at line {line}")
            }
            Self::InvalidCsvColumn {
                line,
                requested,
                columns,
            } => write!(
                f,
                "CSV line {line} has {columns} columns; requested {requested}",
            ),
            Self::InvalidPgm(message) => write!(f, "invalid PGM: {message}"),
            Self::UnsupportedPgmMaxValue(value) => {
                write!(f, "unsupported PGM max value {value}; expected 1..255")
            }
            Self::OpticalVoteShapeMismatch => {
                write!(f, "optical replay frames have mismatched symbol shapes")
            }
            Self::EmptyTrace => write!(f, "replay trace contains no samples"),
            Self::WindowOutsideTrace => {
                write!(f, "requested replay window is outside the trace")
            }
            Self::SampleRateMismatch { expected, actual } => write!(
                f,
                "trace sample rate {actual} Hz does not match decoder {expected} Hz",
            ),
            Self::Signal(error) => write!(f, "signal decoder error: {error:?}"),
        }
    }
}

impl std::error::Error for ReplayError {}

impl From<SignalError> for ReplayError {
    fn from(value: SignalError) -> Self {
        Self::Signal(value)
    }
}

pub fn parse_pcm16_wav(bytes: &[u8]) -> Result<Pcm16Wav, ReplayError> {
    if bytes.len() < 12 {
        return Err(ReplayError::InvalidWav("file is shorter than RIFF header"));
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(ReplayError::InvalidWav("missing RIFF/WAVE signature"));
    }

    let mut offset = 12_usize;
    let mut format = None;
    let mut channels = None;
    let mut sample_rate_hz = None;
    let mut bits_per_sample = None;
    let mut data = None;

    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = read_u32_le(bytes, offset + 4)? as usize;
        let start = offset
            .checked_add(8)
            .ok_or(ReplayError::InvalidWav("chunk offset overflow"))?;
        let end = start
            .checked_add(size)
            .ok_or(ReplayError::InvalidWav("chunk size overflow"))?;
        if end > bytes.len() {
            return Err(ReplayError::InvalidWav("chunk exceeds file length"));
        }

        if id == b"fmt " {
            if size < 16 {
                return Err(ReplayError::InvalidWav("fmt chunk is too short"));
            }
            format = Some(read_u16_le(bytes, start)?);
            channels = Some(read_u16_le(bytes, start + 2)?);
            sample_rate_hz = Some(read_u32_le(bytes, start + 4)?);
            bits_per_sample = Some(read_u16_le(bytes, start + 14)?);
        } else if id == b"data" && data.is_none() {
            data = Some(&bytes[start..end]);
        }

        offset = end + (size & 1);
    }

    let format = format.ok_or(ReplayError::InvalidWav("missing fmt chunk"))?;
    let channels =
        channels.ok_or(ReplayError::InvalidWav("missing channel count"))?;
    let sample_rate_hz =
        sample_rate_hz.ok_or(ReplayError::InvalidWav("missing sample rate"))?;
    let bits_per_sample = bits_per_sample
        .ok_or(ReplayError::InvalidWav("missing bits per sample"))?;
    let data = data.ok_or(ReplayError::InvalidWav("missing data chunk"))?;

    if format != 1 || channels == 0 || bits_per_sample != 16 {
        return Err(ReplayError::UnsupportedWav {
            format,
            channels,
            bits_per_sample,
        });
    }
    if sample_rate_hz == 0 {
        return Err(ReplayError::InvalidWav("sample rate is zero"));
    }

    let frame_bytes = channels as usize * 2;
    if !data.len().is_multiple_of(frame_bytes) {
        return Err(ReplayError::InvalidWav(
            "PCM data is not aligned to whole frames",
        ));
    }

    let mut samples = Vec::with_capacity(data.len() / 2);
    for chunk in data.as_chunks::<2>().0 {
        samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }

    Ok(Pcm16Wav {
        sample_rate_hz,
        channels,
        interleaved_samples: samples,
    })
}

pub fn parse_scalar_csv_column(
    text: &str,
    value_column: usize,
) -> Result<ScalarCsvTrace, ReplayError> {
    let mut values = Vec::new();
    let mut skipped_header = false;

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let columns = line.split(',').map(str::trim).collect::<Vec<_>>();
        let Some(value) = columns.get(value_column) else {
            return Err(ReplayError::InvalidCsvColumn {
                line: line_number,
                requested: value_column,
                columns: columns.len(),
            });
        };

        match value.parse::<f32>() {
            Ok(parsed) if parsed.is_finite() => values.push(parsed),
            _ if values.is_empty() && !skipped_header => {
                skipped_header = true;
            }
            _ => {
                return Err(ReplayError::InvalidCsvLine {
                    line: line_number,
                });
            }
        }
    }

    if values.is_empty() {
        return Err(ReplayError::EmptyTrace);
    }

    Ok(ScalarCsvTrace {
        values,
        skipped_header,
    })
}

pub fn parse_pgm_gray8(bytes: &[u8]) -> Result<PgmGray8, ReplayError> {
    let mut offset = 0_usize;
    let magic = pgm_token(bytes, &mut offset)?;
    if magic != "P5" {
        return Err(ReplayError::InvalidPgm(
            "only binary P5 grayscale is supported",
        ));
    }

    let width = pgm_token(bytes, &mut offset)?
        .parse::<usize>()
        .map_err(|_| ReplayError::InvalidPgm("invalid width"))?;
    let height = pgm_token(bytes, &mut offset)?
        .parse::<usize>()
        .map_err(|_| ReplayError::InvalidPgm("invalid height"))?;
    let max_value = pgm_token(bytes, &mut offset)?
        .parse::<u32>()
        .map_err(|_| ReplayError::InvalidPgm("invalid max value"))?;

    if width == 0 || height == 0 {
        return Err(ReplayError::InvalidPgm(
            "width and height must be non-zero",
        ));
    }
    if !(1..=255).contains(&max_value) {
        return Err(ReplayError::UnsupportedPgmMaxValue(max_value));
    }

    let separator = *bytes
        .get(offset)
        .ok_or(ReplayError::InvalidPgm(
            "missing separator before pixel data",
        ))?;
    if !separator.is_ascii_whitespace() {
        return Err(ReplayError::InvalidPgm(
            "missing whitespace separator before pixel data",
        ));
    }
    offset += 1;

    let expected = width
        .checked_mul(height)
        .ok_or(ReplayError::InvalidPgm("image size overflow"))?;
    let pixels = bytes
        .get(offset..)
        .ok_or(ReplayError::InvalidPgm("missing pixel data"))?;
    if pixels.len() != expected {
        return Err(ReplayError::InvalidPgm(
            "pixel data length does not match dimensions",
        ));
    }

    Ok(PgmGray8 {
        width,
        height,
        max_value: max_value as u8,
        pixels: pixels.to_vec(),
    })
}

pub fn vote_optical_symbols(
    frames: &[Vec<Option<u8>>],
) -> Result<Vec<Option<u8>>, ReplayError> {
    let Some(first) = frames.first() else {
        return Err(ReplayError::EmptyTrace);
    };
    if frames.iter().any(|frame| frame.len() != first.len()) {
        return Err(ReplayError::OpticalVoteShapeMismatch);
    }

    let mut voted = Vec::with_capacity(first.len());
    for index in 0..first.len() {
        let mut zeros = 0_usize;
        let mut ones = 0_usize;

        for frame in frames {
            match frame[index] {
                Some(0) => zeros += 1,
                Some(1) => ones += 1,
                Some(_) => {
                    return Err(ReplayError::Signal(
                        SignalError::InvalidBit(
                            frame[index].expect("matched Some above"),
                        ),
                    ));
                }
                None => {}
            }
        }

        voted.push(match zeros.cmp(&ones) {
            std::cmp::Ordering::Greater => Some(0),
            std::cmp::Ordering::Less => Some(1),
            std::cmp::Ordering::Equal => None,
        });
    }

    Ok(voted)
}

pub fn replay_optical_pgm_sequence(
    pgm_frames: &[Vec<u8>],
    symbol_count: usize,
    grid: OpticalGridConfig,
    perspective: Option<OpticalPerspective>,
    repetition: OpticalRepetitionConfig,
) -> Result<Vec<u8>, ReplayError> {
    if pgm_frames.is_empty() || symbol_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }

    let mut decoded_frames = Vec::with_capacity(pgm_frames.len());
    for bytes in pgm_frames {
        let pgm = parse_pgm_gray8(bytes)?;
        let frame = pgm.to_optical_frame();
        let decoded = decode_optical_cells(
            &frame,
            symbol_count,
            grid,
            perspective,
        )?;
        decoded_frames.push(decoded);
    }

    let voted = vote_optical_symbols(&decoded_frames)?;
    Ok(decode_optical_repetition(&voted, repetition)?)
}

fn pgm_token(
    bytes: &[u8],
    offset: &mut usize,
) -> Result<String, ReplayError> {
    loop {
        while bytes
            .get(*offset)
            .is_some_and(|value| value.is_ascii_whitespace())
        {
            *offset += 1;
        }

        if bytes.get(*offset) == Some(&b'#') {
            while bytes
                .get(*offset)
                .is_some_and(|value| *value != b'\n')
            {
                *offset += 1;
            }
            continue;
        }
        break;
    }

    let start = *offset;
    while bytes.get(*offset).is_some_and(|value| {
        !value.is_ascii_whitespace() && *value != b'#'
    }) {
        *offset += 1;
    }
    if *offset == start {
        return Err(ReplayError::InvalidPgm("missing header token"));
    }

    std::str::from_utf8(&bytes[start..*offset])
        .map(str::to_owned)
        .map_err(|_| ReplayError::InvalidPgm("header is not ASCII"))
}

pub fn replay_acoustic_wav(
    wav_bytes: &[u8],
    channel_index: usize,
    config: AcousticFskConfig,
) -> Result<DecodeResult, ReplayError> {
    let wav = parse_pcm16_wav(wav_bytes)?;
    if wav.sample_rate_hz != config.sample_rate_hz {
        return Err(ReplayError::SampleRateMismatch {
            expected: config.sample_rate_hz,
            actual: wav.sample_rate_hz,
        });
    }
    let samples = wav.channel_f32(channel_index)?;
    Ok(decode_fsk(&samples, config)?)
}

pub fn replay_acoustic_wav_window(
    wav_bytes: &[u8],
    channel_index: usize,
    config: AcousticFskConfig,
    start_sample: usize,
    bit_count: usize,
) -> Result<DecodeResult, ReplayError> {
    if bit_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }
    let wav = parse_pcm16_wav(wav_bytes)?;
    if wav.sample_rate_hz != config.sample_rate_hz {
        return Err(ReplayError::SampleRateMismatch {
            expected: config.sample_rate_hz,
            actual: wav.sample_rate_hz,
        });
    }

    let samples = wav.channel_f32(channel_index)?;
    let samples_per_bit = config.samples_per_bit()?;
    let needed = bit_count
        .checked_mul(samples_per_bit)
        .ok_or(ReplayError::WindowOutsideTrace)?;
    let end = start_sample
        .checked_add(needed)
        .ok_or(ReplayError::WindowOutsideTrace)?;
    let window = samples
        .get(start_sample..end)
        .ok_or(ReplayError::WindowOutsideTrace)?;

    Ok(decode_fsk(window, config)?)
}

pub fn replay_vibration_csv(
    csv_text: &str,
    value_column: usize,
    config: VibrationOokConfig,
) -> Result<Vec<u8>, ReplayError> {
    let trace = parse_scalar_csv_column(csv_text, value_column)?;
    Ok(decode_vibration_ook(&trace.values, config)?)
}

pub fn replay_vibration_csv_window(
    csv_text: &str,
    value_column: usize,
    config: VibrationOokConfig,
    start_sample: usize,
    bit_count: usize,
) -> Result<Vec<u8>, ReplayError> {
    if bit_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }
    let trace = parse_scalar_csv_column(csv_text, value_column)?;
    let samples_per_bit = config.samples_per_bit()?;
    let needed = bit_count
        .checked_mul(samples_per_bit)
        .ok_or(ReplayError::WindowOutsideTrace)?;
    let end = start_sample
        .checked_add(needed)
        .ok_or(ReplayError::WindowOutsideTrace)?;
    let window = trace
        .values
        .get(start_sample..end)
        .ok_or(ReplayError::WindowOutsideTrace)?;
    Ok(decode_vibration_ook(window, config)?)
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, ReplayError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or(ReplayError::InvalidWav("truncated u16 field"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, ReplayError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or(ReplayError::InvalidWav("truncated u32 field"))?;
    Ok(u32::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use signal_frontier::{
        bit_error_count, encode_fsk, encode_vibration_ook,
    };

    fn pcm16_wav(sample_rate_hz: u32, samples: &[f32]) -> Vec<u8> {
        let data_bytes = (samples.len() * 2) as u32;
        let riff_size = 36 + data_bytes;
        let mut out = Vec::with_capacity(44 + data_bytes as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_size.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&sample_rate_hz.to_le_bytes());
        out.extend_from_slice(&(sample_rate_hz * 2).to_le_bytes());
        out.extend_from_slice(&2_u16.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_bytes.to_le_bytes());
        for &sample in samples {
            let quantized = (sample.clamp(-1.0, 0.9999695) * 32768.0)
                .round() as i16;
            out.extend_from_slice(&quantized.to_le_bytes());
        }
        out
    }

    fn pgm_from_frame(frame: &OpticalGrayFrame) -> Vec<u8> {
        let mut out = format!(
            "P5\n# sanpham3 fixture\n{} {}\n255\n",
            frame.width,
            frame.height,
        )
        .into_bytes();
        out.extend(
            frame
                .pixels
                .iter()
                .map(|value| {
                    (value.clamp(0.0, 1.0) * 255.0).round() as u8
                }),
        );
        out
    }

    #[test]
    fn pgm_parser_and_optical_sequence_replay_roundtrip_bits() {
        use signal_frontier::{
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![1, 0, 1, 1, 0, 0, 1, 0];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let frame = render_optical_cells(&symbols, grid).unwrap();
        let pgm = pgm_from_frame(&frame);

        let parsed = parse_pgm_gray8(&pgm).unwrap();
        assert_eq!(parsed.width, frame.width);
        assert_eq!(parsed.height, frame.height);

        let decoded = replay_optical_pgm_sequence(
            &[pgm.clone(), pgm],
            symbols.len(),
            grid,
            None,
            repetition,
        )
        .unwrap();

        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn optical_symbol_vote_uses_majority_and_preserves_ties_as_erasure() {
        let voted = vote_optical_symbols(&[
            vec![Some(1), Some(0), Some(1), None],
            vec![Some(1), Some(1), Some(0), None],
            vec![None, Some(0), None, None],
        ])
        .unwrap();

        assert_eq!(
            voted,
            vec![Some(1), Some(0), None, None],
        );
    }

    #[test]
    fn wav_parser_and_acoustic_replay_roundtrip_reference_bits() {
        let bits = vec![1, 0, 1, 1, 0, 0, 1, 0];
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        let samples = encode_fsk(&bits, config).unwrap();
        let wav = pcm16_wav(config.sample_rate_hz, &samples);

        let parsed = parse_pcm16_wav(&wav).unwrap();
        assert_eq!(parsed.sample_rate_hz, 48_000);
        assert_eq!(parsed.channels, 1);
        assert_eq!(parsed.frames(), samples.len());

        let decoded = replay_acoustic_wav(&wav, 0, config).unwrap();
        assert_eq!(bit_error_count(&bits, &decoded.bits), 0);
    }

    #[test]
    fn acoustic_window_replay_ignores_leading_and_trailing_samples() {
        let bits = vec![1, 0, 1, 0];
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        let encoded = encode_fsk(&bits, config).unwrap();
        let prefix = vec![0.0_f32; 137];
        let suffix = vec![0.0_f32; 91];
        let mut samples = prefix.clone();
        samples.extend_from_slice(&encoded);
        samples.extend_from_slice(&suffix);
        let wav = pcm16_wav(config.sample_rate_hz, &samples);

        let decoded = replay_acoustic_wav_window(
            &wav,
            0,
            config,
            prefix.len(),
            bits.len(),
        )
        .unwrap();

        assert_eq!(bit_error_count(&bits, &decoded.bits), 0);
    }

    #[test]
    fn vibration_window_replay_ignores_leading_and_trailing_samples() {
        let bits = vec![1, 0, 1, 1];
        let config = VibrationOokConfig::surface_2_5bps();
        let encoded = encode_vibration_ook(&bits, config).unwrap();
        let prefix = vec![0.0_f32; 23];
        let suffix = vec![0.0_f32; 17];
        let mut samples = prefix.clone();
        samples.extend_from_slice(&encoded);
        samples.extend_from_slice(&suffix);

        let mut csv = String::from("index,value\n");
        for (index, sample) in samples.iter().enumerate() {
            csv.push_str(&format!("{index},{sample}\n"));
        }

        let decoded = replay_vibration_csv_window(
            &csv,
            1,
            config,
            prefix.len(),
            bits.len(),
        )
        .unwrap();

        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn wav_replay_rejects_sample_rate_mismatch() {
        let bits = vec![1, 0];
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        let samples = encode_fsk(&bits, config).unwrap();
        let wav = pcm16_wav(44_100, &samples);

        assert!(matches!(
            replay_acoustic_wav(&wav, 0, config),
            Err(ReplayError::SampleRateMismatch {
                expected: 48_000,
                actual: 44_100,
            }),
        ));
    }

    #[test]
    fn csv_parser_accepts_header_comments_and_selected_column() {
        let csv = "# captured sensor\ntime_ms,value\n0,0.1\n5,0.2\n";
        let parsed = parse_scalar_csv_column(csv, 1).unwrap();
        assert_eq!(parsed.values, vec![0.1, 0.2]);
        assert!(parsed.skipped_header);
    }

    #[test]
    fn vibration_csv_replay_roundtrips_reference_bits() {
        let bits = vec![1, 0, 1, 0, 1, 1, 0, 0];
        let config = VibrationOokConfig::surface_2_5bps();
        let samples = encode_vibration_ook(&bits, config).unwrap();
        let mut csv = String::from("index,value\n");
        for (index, sample) in samples.iter().enumerate() {
            csv.push_str(&format!("{index},{sample}\n"));
        }

        let decoded =
            replay_vibration_csv(&csv, 1, config).unwrap();
        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn csv_parser_rejects_bad_numeric_row_after_header() {
        let csv = "time,value\n0,0.1\n1,nope\n";
        assert!(matches!(
            parse_scalar_csv_column(csv, 1),
            Err(ReplayError::InvalidCsvLine { line: 3 }),
        ));
    }
}
