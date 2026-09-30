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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Y4mGrayVideo {
    pub width: usize,
    pub height: usize,
    pub fps_numerator: u32,
    pub fps_denominator: u32,
    pub chroma: String,
    pub luma_frames: Vec<Vec<u8>>,
}

impl Y4mGrayVideo {
    pub fn frame_as_optical(
        &self,
        index: usize,
    ) -> Result<OpticalGrayFrame, ReplayError> {
        let pixels = self
            .luma_frames
            .get(index)
            .ok_or(ReplayError::VideoFrameWindowOutside)?;
        Ok(OpticalGrayFrame {
            width: self.width,
            height: self.height,
            pixels: pixels
                .iter()
                .map(|&value| value as f32 / 255.0)
                .collect(),
        })
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
    InvalidY4m(&'static str),
    UnsupportedY4mChroma(String),
    VideoFrameWindowOutside,
    OpticalVoteShapeMismatch,
    OpticalRegistrationFailed(&'static str),
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
            Self::InvalidY4m(message) => {
                write!(f, "invalid Y4M: {message}")
            }
            Self::UnsupportedY4mChroma(chroma) => {
                write!(f, "unsupported Y4M chroma mode {chroma}")
            }
            Self::VideoFrameWindowOutside => {
                write!(f, "requested video frame window is outside the capture")
            }
            Self::OpticalVoteShapeMismatch => {
                write!(f, "optical replay frames have mismatched symbol shapes")
            }
            Self::OpticalRegistrationFailed(message) => {
                write!(f, "optical registration failed: {message}")
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

pub fn parse_y4m_gray_video(
    bytes: &[u8],
) -> Result<Y4mGrayVideo, ReplayError> {
    let mut offset = 0_usize;
    let header = y4m_line(bytes, &mut offset)?;
    let header_text = std::str::from_utf8(header)
        .map_err(|_| ReplayError::InvalidY4m("header is not ASCII"))?;
    let mut tokens = header_text.split_ascii_whitespace();

    if tokens.next() != Some("YUV4MPEG2") {
        return Err(ReplayError::InvalidY4m(
            "missing YUV4MPEG2 signature",
        ));
    }

    let mut width = None;
    let mut height = None;
    let mut fps = None;
    let mut chroma = None;

    for token in tokens {
        if let Some(value) = token.strip_prefix('W') {
            width = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| ReplayError::InvalidY4m("invalid width"))?,
            );
        } else if let Some(value) = token.strip_prefix('H') {
            height = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| ReplayError::InvalidY4m("invalid height"))?,
            );
        } else if let Some(value) = token.strip_prefix('F') {
            let (numerator, denominator) = value
                .split_once(':')
                .ok_or(ReplayError::InvalidY4m(
                    "invalid frame-rate ratio",
                ))?;
            fps = Some((
                numerator
                    .parse::<u32>()
                    .map_err(|_| ReplayError::InvalidY4m(
                        "invalid frame-rate numerator",
                    ))?,
                denominator
                    .parse::<u32>()
                    .map_err(|_| ReplayError::InvalidY4m(
                        "invalid frame-rate denominator",
                    ))?,
            ));
        } else if let Some(value) = token.strip_prefix('C') {
            chroma = Some(value.to_owned());
        }
    }

    let width = width.ok_or(ReplayError::InvalidY4m("missing width"))?;
    let height = height.ok_or(ReplayError::InvalidY4m("missing height"))?;
    if width == 0 || height == 0 {
        return Err(ReplayError::InvalidY4m(
            "width and height must be non-zero",
        ));
    }

    let (fps_numerator, fps_denominator) =
        fps.ok_or(ReplayError::InvalidY4m("missing frame rate"))?;
    if fps_numerator == 0 || fps_denominator == 0 {
        return Err(ReplayError::InvalidY4m(
            "frame-rate ratio must be non-zero",
        ));
    }

    let chroma = chroma.unwrap_or_else(|| "420jpeg".to_owned());
    let y_bytes = width
        .checked_mul(height)
        .ok_or(ReplayError::InvalidY4m("luma size overflow"))?;
    let chroma_bytes = y4m_chroma_bytes(width, height, &chroma)?;
    let frame_bytes = y_bytes
        .checked_add(chroma_bytes)
        .ok_or(ReplayError::InvalidY4m("frame size overflow"))?;

    let mut luma_frames = Vec::new();
    while offset < bytes.len() {
        let frame_header = y4m_line(bytes, &mut offset)?;
        let frame_header_text = std::str::from_utf8(frame_header)
            .map_err(|_| ReplayError::InvalidY4m(
                "frame header is not ASCII",
            ))?;
        if !frame_header_text
            .split_ascii_whitespace()
            .next()
            .is_some_and(|token| token == "FRAME")
        {
            return Err(ReplayError::InvalidY4m(
                "missing FRAME header",
            ));
        }

        let end = offset
            .checked_add(frame_bytes)
            .ok_or(ReplayError::InvalidY4m(
                "frame payload offset overflow",
            ))?;
        if end > bytes.len() {
            return Err(ReplayError::InvalidY4m(
                "truncated frame payload",
            ));
        }

        luma_frames.push(bytes[offset..offset + y_bytes].to_vec());
        offset = end;
    }

    if luma_frames.is_empty() {
        return Err(ReplayError::EmptyTrace);
    }

    Ok(Y4mGrayVideo {
        width,
        height,
        fps_numerator,
        fps_denominator,
        chroma,
        luma_frames,
    })
}

pub fn replay_optical_y4m_registered_scaled(
    y4m_bytes: &[u8],
    symbol_count: usize,
    grid: OpticalGridConfig,
    repetition: OpticalRepetitionConfig,
    start_frame: usize,
    frame_count: usize,
) -> Result<Vec<u8>, ReplayError> {
    if symbol_count == 0 || frame_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }

    let video = parse_y4m_gray_video(y4m_bytes)?;
    let end = start_frame
        .checked_add(frame_count)
        .ok_or(ReplayError::VideoFrameWindowOutside)?;
    if end > video.luma_frames.len() {
        return Err(ReplayError::VideoFrameWindowOutside);
    }

    let mut decoded_frames = Vec::with_capacity(frame_count);
    for index in start_frame..end {
        let source = video.frame_as_optical(index)?;
        decoded_frames.push(
            decode_optical_cells_best_quarter_turn(
                &source,
                symbol_count,
                grid,
                repetition,
            )?,
        );
    }

    let voted = vote_optical_symbols(&decoded_frames)?;
    Ok(decode_optical_repetition(&voted, repetition)?)
}

fn rotate_optical_frame_quarter_turns(
    frame: &OpticalGrayFrame,
    quarter_turns_clockwise: u8,
) -> OpticalGrayFrame {
    match quarter_turns_clockwise % 4 {
        0 => frame.clone(),
        1 => {
            let mut pixels = vec![0.0_f32; frame.pixels.len()];
            let width = frame.height;
            let height = frame.width;
            for y in 0..frame.height {
                for x in 0..frame.width {
                    let dst_x = frame.height - 1 - y;
                    let dst_y = x;
                    pixels[dst_y * width + dst_x] =
                        frame.pixels[y * frame.width + x];
                }
            }
            OpticalGrayFrame {
                width,
                height,
                pixels,
            }
        }
        2 => {
            let mut pixels = vec![0.0_f32; frame.pixels.len()];
            for y in 0..frame.height {
                for x in 0..frame.width {
                    let dst_x = frame.width - 1 - x;
                    let dst_y = frame.height - 1 - y;
                    pixels[dst_y * frame.width + dst_x] =
                        frame.pixels[y * frame.width + x];
                }
            }
            OpticalGrayFrame {
                width: frame.width,
                height: frame.height,
                pixels,
            }
        }
        _ => {
            let mut pixels = vec![0.0_f32; frame.pixels.len()];
            let width = frame.height;
            let height = frame.width;
            for y in 0..frame.height {
                for x in 0..frame.width {
                    let dst_x = y;
                    let dst_y = frame.width - 1 - x;
                    pixels[dst_y * width + dst_x] =
                        frame.pixels[y * frame.width + x];
                }
            }
            OpticalGrayFrame {
                width,
                height,
                pixels,
            }
        }
    }
}

struct OpticalQuarterTurnCandidate {
    marker_score: f32,
    balance: f32,
    disagreements: usize,
    resolved: usize,
    decoded: Vec<Option<u8>>,
}

fn decode_optical_cells_best_quarter_turn(
    source: &OpticalGrayFrame,
    symbol_count: usize,
    grid: OpticalGridConfig,
    repetition: OpticalRepetitionConfig,
) -> Result<Vec<Option<u8>>, ReplayError> {
    repetition.validate()?;

    let mut best: Option<OpticalQuarterTurnCandidate> = None;
    let mut last_error = None;

    for quarter_turns in 0_u8..4 {
        let rotated =
            rotate_optical_frame_quarter_turns(source, quarter_turns);
        let attempt = register_optical_translation_scale(
            &rotated,
            symbol_count,
            grid,
        )
        .and_then(|(registered, info)| {
            let decoded = decode_optical_cells(
                &registered,
                symbol_count,
                grid,
                None,
            )?;
            let resolved = decoded
                .iter()
                .filter(|value| value.is_some())
                .count();
            let disagreements =
                optical_repetition_disagreements(
                    &decoded,
                    repetition,
                );
            let balance =
                (info.scale_x / info.scale_y).ln().abs();
            let marker_score =
                optical_orientation_marker_score(
                    &registered,
                    grid,
                )?;
            Ok(OpticalQuarterTurnCandidate {
                marker_score,
                balance,
                disagreements,
                resolved,
                decoded,
            })
        });

        match attempt {
            Ok(candidate) => {
                let replace = best
                    .as_ref()
                    .is_none_or(|current| {
                        let marker_delta =
                            candidate.marker_score -
                                current.marker_score;
                        if marker_delta.abs() > 0.01 {
                            marker_delta > 0.0
                        } else if
                            (candidate.balance - current.balance).abs() >
                                0.02
                        {
                            candidate.balance < current.balance
                        } else if
                            candidate.disagreements !=
                                current.disagreements
                        {
                            candidate.disagreements <
                                current.disagreements
                        } else {
                            candidate.resolved > current.resolved
                        }
                    });
                if replace {
                    best = Some(candidate);
                }
            }
            Err(error) => last_error = Some(error),
        }
    }

    best.map(|candidate| candidate.decoded)
        .ok_or_else(|| {
            last_error.unwrap_or(
                ReplayError::OpticalRegistrationFailed(
                    "all quarter-turn registrations failed",
                ),
            )
        })
}

fn optical_orientation_marker_score(
    frame: &OpticalGrayFrame,
    grid: OpticalGridConfig,
) -> Result<f32, ReplayError> {
    if frame.width == 0
        || frame.height == 0
        || frame.pixels.len() != frame.width.saturating_mul(frame.height)
    {
        return Err(ReplayError::OpticalRegistrationFailed(
            "registered frame dimensions are invalid",
        ));
    }
    grid.validate()?;

    let cell = grid.cell_pixels;
    if frame.width < cell || frame.height < cell {
        return Err(ReplayError::OpticalRegistrationFailed(
            "registered frame is too small for orientation finder",
        ));
    }

    let mean_block = |x0: usize, y0: usize| -> f32 {
        let mut sum = 0.0_f32;
        for y in y0..y0 + cell {
            let start = y * frame.width + x0;
            sum += frame.pixels[start..start + cell]
                .iter()
                .sum::<f32>();
        }
        sum / (cell * cell) as f32
    };

    let top_left = mean_block(0, 0);
    let top_right = mean_block(frame.width - cell, 0);
    let bottom_left = mean_block(0, frame.height - cell);
    let bottom_right = mean_block(
        frame.width - cell,
        frame.height - cell,
    );

    Ok(
        top_left
            - top_right
                .max(bottom_left)
                .max(bottom_right),
    )
}

fn optical_repetition_disagreements(
    symbols: &[Option<u8>],
    repetition: OpticalRepetitionConfig,
) -> usize {
    symbols
        .chunks(repetition.repeats_per_bit)
        .map(|chunk| {
            let zeros = chunk
                .iter()
                .filter(|value| matches!(value, Some(0)))
                .count();
            let ones = chunk
                .iter()
                .filter(|value| matches!(value, Some(1)))
                .count();
            zeros.min(ones)
        })
        .sum()
}

fn y4m_line<'a>(
    bytes: &'a [u8],
    offset: &mut usize,
) -> Result<&'a [u8], ReplayError> {
    if *offset >= bytes.len() {
        return Err(ReplayError::InvalidY4m("missing line"));
    }

    let relative_end = bytes[*offset..]
        .iter()
        .position(|&value| value == b'\n')
        .ok_or(ReplayError::InvalidY4m(
            "unterminated header line",
        ))?;
    let end = *offset + relative_end;
    let mut line = &bytes[*offset..end];
    if line.last() == Some(&b'\r') {
        line = &line[..line.len() - 1];
    }
    *offset = end + 1;
    Ok(line)
}

fn y4m_chroma_bytes(
    width: usize,
    height: usize,
    chroma: &str,
) -> Result<usize, ReplayError> {
    let chroma_pixels = if chroma == "mono" {
        0
    } else if chroma.starts_with("420") {
        let cw = width.div_ceil(2);
        let ch = height.div_ceil(2);
        cw.checked_mul(ch)
            .and_then(|plane| plane.checked_mul(2))
            .ok_or(ReplayError::InvalidY4m("4:2:0 size overflow"))?
    } else if chroma.starts_with("422") {
        let cw = width.div_ceil(2);
        cw.checked_mul(height)
            .and_then(|plane| plane.checked_mul(2))
            .ok_or(ReplayError::InvalidY4m("4:2:2 size overflow"))?
    } else if chroma.starts_with("444") {
        width
            .checked_mul(height)
            .and_then(|plane| plane.checked_mul(2))
            .ok_or(ReplayError::InvalidY4m("4:4:4 size overflow"))?
    } else {
        return Err(ReplayError::UnsupportedY4mChroma(
            chroma.to_owned(),
        ));
    };

    Ok(chroma_pixels)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpticalTranslationRegistration {
    pub source_origin_x: usize,
    pub source_origin_y: usize,
    pub registered_width: usize,
    pub registered_height: usize,
}

pub fn register_optical_translation(
    frame: &OpticalGrayFrame,
    symbol_count: usize,
    grid: OpticalGridConfig,
) -> Result<(OpticalGrayFrame, OpticalTranslationRegistration), ReplayError> {
    if symbol_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }

    grid.validate()?;
    if frame.width == 0
        || frame.height == 0
        || frame.pixels.len() != frame.width.saturating_mul(frame.height)
    {
        return Err(ReplayError::OpticalRegistrationFailed(
            "source frame dimensions are invalid",
        ));
    }

    let rows = symbol_count.div_ceil(grid.columns);
    let quiet_pixels = grid
        .quiet_zone_cells
        .checked_mul(grid.cell_pixels)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "quiet-zone size overflow",
        ))?;
    let registered_width = grid
        .columns
        .checked_add(grid.quiet_zone_cells.saturating_mul(2))
        .and_then(|cells| cells.checked_mul(grid.cell_pixels))
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "registered width overflow",
        ))?;
    let registered_height = rows
        .checked_add(grid.quiet_zone_cells.saturating_mul(2))
        .and_then(|cells| cells.checked_mul(grid.cell_pixels))
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "registered height overflow",
        ))?;

    let background = (grid.zero_level * 0.25).clamp(0.0, 1.0);
    let activation_threshold =
        background + (grid.zero_level - background) * 0.50;

    let mut min_x = usize::MAX;
    let mut min_y = usize::MAX;
    let mut found = false;

    for y in 0..frame.height {
        let row_start = y * frame.width;
        for x in 0..frame.width {
            if frame.pixels[row_start + x] >= activation_threshold {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                found = true;
            }
        }
    }

    if !found {
        return Err(ReplayError::OpticalRegistrationFailed(
            "no active optical cells found",
        ));
    }
    if min_x < quiet_pixels || min_y < quiet_pixels {
        return Err(ReplayError::OpticalRegistrationFailed(
            "active grid is too close to source-frame edge",
        ));
    }

    let source_origin_x = min_x - quiet_pixels;
    let source_origin_y = min_y - quiet_pixels;
    let source_end_x = source_origin_x
        .checked_add(registered_width)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "registered crop x overflow",
        ))?;
    let source_end_y = source_origin_y
        .checked_add(registered_height)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "registered crop y overflow",
        ))?;

    if source_end_x > frame.width || source_end_y > frame.height {
        return Err(ReplayError::OpticalRegistrationFailed(
            "registered crop exceeds source frame",
        ));
    }

    let mut pixels =
        Vec::with_capacity(registered_width.saturating_mul(registered_height));
    for y in source_origin_y..source_end_y {
        let start = y * frame.width + source_origin_x;
        let end = start + registered_width;
        pixels.extend_from_slice(&frame.pixels[start..end]);
    }

    Ok((
        OpticalGrayFrame {
            width: registered_width,
            height: registered_height,
            pixels,
        },
        OpticalTranslationRegistration {
            source_origin_x,
            source_origin_y,
            registered_width,
            registered_height,
        },
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpticalScaleRegistration {
    pub source_origin_x: usize,
    pub source_origin_y: usize,
    pub source_width: usize,
    pub source_height: usize,
    pub scale_x: f32,
    pub scale_y: f32,
}

fn optical_scale_active_bbox(
    frame: &OpticalGrayFrame,
    symbol_count: usize,
    cell_pixels: usize,
    data_width: usize,
    data_height: usize,
) -> Result<(usize, usize, usize, usize, f32, f32), ReplayError> {
    const LEVELS: usize = 256;

    let mut counts = [0_usize; LEVELS];
    let mut min_x_by_level = [usize::MAX; LEVELS];
    let mut min_y_by_level = [usize::MAX; LEVELS];
    let mut max_x_by_level = [0_usize; LEVELS];
    let mut max_y_by_level = [0_usize; LEVELS];

    for y in 0..frame.height {
        let row_start = y * frame.width;
        for x in 0..frame.width {
            let level = (frame.pixels[row_start + x]
                .clamp(0.0, 1.0)
                * 255.0)
                .round() as usize;
            counts[level] += 1;
            min_x_by_level[level] =
                min_x_by_level[level].min(x);
            min_y_by_level[level] =
                min_y_by_level[level].min(y);
            max_x_by_level[level] =
                max_x_by_level[level].max(x);
            max_y_by_level[level] =
                max_y_by_level[level].max(y);
        }
    }

    let expected_cell_area = symbol_count
        .checked_mul(cell_pixels)
        .and_then(|value| value.checked_mul(cell_pixels))
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "expected active optical area overflow",
        ))? as f32;

    let mut cumulative_count = 0_usize;
    let mut cumulative_min_x = usize::MAX;
    let mut cumulative_min_y = usize::MAX;
    let mut cumulative_max_x = 0_usize;
    let mut cumulative_max_y = 0_usize;

    let mut best: Option<(
        f32,
        usize,
        usize,
        usize,
        usize,
        usize,
        f32,
        f32,
    )> = None;

    for threshold in (1..LEVELS).rev() {
        if counts[threshold] > 0 {
            cumulative_count += counts[threshold];
            cumulative_min_x =
                cumulative_min_x.min(min_x_by_level[threshold]);
            cumulative_min_y =
                cumulative_min_y.min(min_y_by_level[threshold]);
            cumulative_max_x =
                cumulative_max_x.max(max_x_by_level[threshold]);
            cumulative_max_y =
                cumulative_max_y.max(max_y_by_level[threshold]);
        }

        if cumulative_count == 0
            || cumulative_min_x == usize::MAX
            || cumulative_min_y == usize::MAX
        {
            continue;
        }

        let observed_width =
            cumulative_max_x - cumulative_min_x + 1;
        let observed_height =
            cumulative_max_y - cumulative_min_y + 1;
        let scale_x =
            observed_width as f32 / data_width as f32;
        let scale_y =
            observed_height as f32 / data_height as f32;

        if !(0.50..=3.00).contains(&scale_x)
            || !(0.50..=3.00).contains(&scale_y)
        {
            continue;
        }

        let expected_active_area =
            expected_cell_area * scale_x * scale_y;
        if expected_active_area <= 0.0 {
            continue;
        }

        let coverage =
            cumulative_count as f32 / expected_active_area;
        let balance =
            (scale_x / scale_y).ln().abs();
        let score = balance + (coverage - 1.0).abs();

        let replace = best.as_ref().is_none_or(|current| {
            score < current.0 - 1.0e-6
                || ((score - current.0).abs() <= 1.0e-6
                    && threshold < current.1)
        });

        if replace {
            best = Some((
                score,
                threshold,
                cumulative_min_x,
                cumulative_min_y,
                cumulative_max_x,
                cumulative_max_y,
                scale_x,
                scale_y,
            ));
        }
    }

    let Some((
        _,
        _,
        min_x,
        min_y,
        max_x,
        max_y,
        scale_x,
        scale_y,
    )) = best
    else {
        return Err(ReplayError::OpticalRegistrationFailed(
            "no activation threshold produced bounded optical geometry",
        ));
    };

    Ok((
        min_x,
        min_y,
        max_x,
        max_y,
        scale_x,
        scale_y,
    ))
}

pub fn register_optical_translation_scale(
    frame: &OpticalGrayFrame,
    symbol_count: usize,
    grid: OpticalGridConfig,
) -> Result<(OpticalGrayFrame, OpticalScaleRegistration), ReplayError> {
    if symbol_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }

    grid.validate()?;
    if frame.width == 0
        || frame.height == 0
        || frame.pixels.len() != frame.width.saturating_mul(frame.height)
    {
        return Err(ReplayError::OpticalRegistrationFailed(
            "source frame dimensions are invalid",
        ));
    }

    let rows = symbol_count.div_ceil(grid.columns);
    let quiet_pixels = grid
        .quiet_zone_cells
        .checked_mul(grid.cell_pixels)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "quiet-zone size overflow",
        ))?;
    let data_width = grid
        .columns
        .checked_mul(grid.cell_pixels)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "data width overflow",
        ))?;
    let data_height = rows
        .checked_mul(grid.cell_pixels)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "data height overflow",
        ))?;
    let target_width = grid
        .columns
        .checked_add(grid.quiet_zone_cells.saturating_mul(2))
        .and_then(|cells| cells.checked_mul(grid.cell_pixels))
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "target width overflow",
        ))?;
    let target_height = rows
        .checked_add(grid.quiet_zone_cells.saturating_mul(2))
        .and_then(|cells| cells.checked_mul(grid.cell_pixels))
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "target height overflow",
        ))?;

    let (
        min_x,
        min_y,
        _max_x,
        _max_y,
        scale_x,
        scale_y,
    ) = optical_scale_active_bbox(
        frame,
        symbol_count,
        grid.cell_pixels,
        data_width,
        data_height,
    )?;

    // Camera pipelines can resize/crop the luma plane differently on the two
    // axes. Keep X/Y scale independent while preserving the same bounded
    // research range enforced above.
    let scaled_quiet_x =
        (quiet_pixels as f32 * scale_x).round() as usize;
    let scaled_quiet_y =
        (quiet_pixels as f32 * scale_y).round() as usize;

    // Inclusive active-pixel bounds plus independent scale rounding can move
    // an otherwise complete quiet-zone crop by one source pixel. Permit only
    // a tiny two-pixel edge clamp; anything larger remains a hard failure so
    // real camera truncation cannot be hidden as "rounding".
    const EDGE_ROUNDING_TOLERANCE: usize = 2;

    let left_clip = scaled_quiet_x.saturating_sub(min_x);
    let top_clip = scaled_quiet_y.saturating_sub(min_y);
    if left_clip > EDGE_ROUNDING_TOLERANCE
        || top_clip > EDGE_ROUNDING_TOLERANCE
    {
        return Err(ReplayError::OpticalRegistrationFailed(
            "scaled grid is too close to source-frame edge",
        ));
    }

    let source_origin_x = min_x.saturating_sub(scaled_quiet_x);
    let source_origin_y = min_y.saturating_sub(scaled_quiet_y);
    let nominal_source_width =
        (target_width as f32 * scale_x).round() as usize;
    let nominal_source_height =
        (target_height as f32 * scale_y).round() as usize;

    if nominal_source_width == 0 || nominal_source_height == 0 {
        return Err(ReplayError::OpticalRegistrationFailed(
            "scaled crop dimensions are zero",
        ));
    }

    let mut source_width = nominal_source_width
        .checked_sub(left_clip)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "left edge clamp exceeds scaled crop width",
        ))?;
    let mut source_height = nominal_source_height
        .checked_sub(top_clip)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "top edge clamp exceeds scaled crop height",
        ))?;

    let source_end_x = source_origin_x
        .checked_add(source_width)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "scaled crop x overflow",
        ))?;
    let source_end_y = source_origin_y
        .checked_add(source_height)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "scaled crop y overflow",
        ))?;

    let right_clip = source_end_x.saturating_sub(frame.width);
    let bottom_clip = source_end_y.saturating_sub(frame.height);
    if right_clip > EDGE_ROUNDING_TOLERANCE
        || bottom_clip > EDGE_ROUNDING_TOLERANCE
    {
        return Err(ReplayError::OpticalRegistrationFailed(
            "scaled crop exceeds source frame",
        ));
    }

    source_width = source_width
        .checked_sub(right_clip)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "right edge clamp exceeds scaled crop width",
        ))?;
    source_height = source_height
        .checked_sub(bottom_clip)
        .ok_or(ReplayError::OpticalRegistrationFailed(
            "bottom edge clamp exceeds scaled crop height",
        ))?;

    if source_width == 0 || source_height == 0 {
        return Err(ReplayError::OpticalRegistrationFailed(
            "edge-clamped crop dimensions are zero",
        ));
    }

    let source_end_x = source_origin_x + source_width;
    let source_end_y = source_origin_y + source_height;

    let mut pixels =
        Vec::with_capacity(target_width.saturating_mul(target_height));
    for target_y in 0..target_height {
        let source_y = source_origin_y
            + ((target_y as f32 + 0.5)
                * source_height as f32
                / target_height as f32)
                .floor() as usize;
        let source_y = source_y.min(source_end_y - 1);

        for target_x in 0..target_width {
            let source_x = source_origin_x
                + ((target_x as f32 + 0.5)
                    * source_width as f32
                    / target_width as f32)
                    .floor() as usize;
            let source_x = source_x.min(source_end_x - 1);
            pixels.push(frame.pixels[source_y * frame.width + source_x]);
        }
    }

    Ok((
        OpticalGrayFrame {
            width: target_width,
            height: target_height,
            pixels,
        },
        OpticalScaleRegistration {
            source_origin_x,
            source_origin_y,
            source_width,
            source_height,
            scale_x,
            scale_y,
        },
    ))
}

pub fn replay_optical_pgm_sequence_registered_scaled(
    pgm_frames: &[Vec<u8>],
    symbol_count: usize,
    grid: OpticalGridConfig,
    repetition: OpticalRepetitionConfig,
) -> Result<Vec<u8>, ReplayError> {
    if pgm_frames.is_empty() || symbol_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }

    let mut decoded_frames = Vec::with_capacity(pgm_frames.len());
    for bytes in pgm_frames {
        let pgm = parse_pgm_gray8(bytes)?;
        let source = pgm.to_optical_frame();
        let (registered, _) =
            register_optical_translation_scale(&source, symbol_count, grid)?;
        let decoded = decode_optical_cells(
            &registered,
            symbol_count,
            grid,
            None,
        )?;
        decoded_frames.push(decoded);
    }

    let voted = vote_optical_symbols(&decoded_frames)?;
    Ok(decode_optical_repetition(&voted, repetition)?)
}

pub fn replay_optical_pgm_sequence_registered(
    pgm_frames: &[Vec<u8>],
    symbol_count: usize,
    grid: OpticalGridConfig,
    repetition: OpticalRepetitionConfig,
) -> Result<Vec<u8>, ReplayError> {
    if pgm_frames.is_empty() || symbol_count == 0 {
        return Err(ReplayError::EmptyTrace);
    }

    let mut decoded_frames = Vec::with_capacity(pgm_frames.len());
    for bytes in pgm_frames {
        let pgm = parse_pgm_gray8(bytes)?;
        let source = pgm.to_optical_frame();
        let (registered, _) =
            register_optical_translation(&source, symbol_count, grid)?;
        let decoded = decode_optical_cells(
            &registered,
            symbol_count,
            grid,
            None,
        )?;
        decoded_frames.push(decoded);
    }

    let voted = vote_optical_symbols(&decoded_frames)?;
    Ok(decode_optical_repetition(&voted, repetition)?)
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

    fn y4m_from_frames(
        width: usize,
        height: usize,
        frames: &[OpticalGrayFrame],
    ) -> Vec<u8> {
        let mut out =
            format!("YUV4MPEG2 W{width} H{height} F30:1 Ip C420jpeg\n")
                .into_bytes();
        let chroma_plane = width.div_ceil(2) * height.div_ceil(2);

        for frame in frames {
            assert_eq!(frame.width, width);
            assert_eq!(frame.height, height);
            out.extend_from_slice(b"FRAME\n");
            out.extend(frame.pixels.iter().map(|value| {
                (value.clamp(0.0, 1.0) * 255.0).round() as u8
            }));
            out.extend(std::iter::repeat_n(128_u8, chroma_plane * 2));
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

    fn embed_optical_frame(
        frame: &OpticalGrayFrame,
        canvas_width: usize,
        canvas_height: usize,
        offset_x: usize,
        offset_y: usize,
    ) -> OpticalGrayFrame {
        assert!(offset_x + frame.width <= canvas_width);
        assert!(offset_y + frame.height <= canvas_height);

        let mut canvas = OpticalGrayFrame {
            width: canvas_width,
            height: canvas_height,
            pixels: vec![0.025_f32; canvas_width * canvas_height],
        };

        for y in 0..canvas_height {
            for x in 0..canvas_width {
                let jitter =
                    (((x * 17 + y * 31) % 11) as f32 - 5.0) * 0.0015;
                canvas.pixels[y * canvas_width + x] =
                    (canvas.pixels[y * canvas_width + x] + jitter)
                        .clamp(0.0, 0.06);
            }
        }

        for y in 0..frame.height {
            let dst = (offset_y + y) * canvas_width + offset_x;
            let src = y * frame.width;
            canvas.pixels[dst..dst + frame.width]
                .copy_from_slice(&frame.pixels[src..src + frame.width]);
        }

        canvas
    }

    fn resize_optical_nearest(
        frame: &OpticalGrayFrame,
        numerator: usize,
        denominator: usize,
    ) -> OpticalGrayFrame {
        assert!(numerator > 0);
        assert!(denominator > 0);
        let width = frame.width * numerator / denominator;
        let height = frame.height * numerator / denominator;
        assert!(width > 0 && height > 0);

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

        OpticalGrayFrame {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn optical_scale_registration_ignores_low_level_camera_halo() {
        use signal_frontier::{
            decode_optical_cells, decode_optical_repetition,
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 0, 1,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let base = render_optical_cells(&symbols, grid).unwrap();

        let mut camera_like = base.clone();
        for value in &mut camera_like.pixels {
            *value = if *value > 0.50 {
                0.82
            } else if *value > 0.15 {
                0.21
            } else if *value > 0.07 {
                0.13
            } else {
                0.07
            };
        }

        let mut source = embed_optical_frame(
            &camera_like,
            camera_like.width + 80,
            camera_like.height + 80,
            40,
            40,
        );

        // This halo is above the historical fixed activation threshold but
        // below the logical-zero cluster. Geometry+coverage selection must
        // reject it as data.
        for y in 20..source.height - 20 {
            source.pixels[y * source.width + 20] = 0.13;
        }

        let (registered, info) =
            register_optical_translation_scale(
                &source,
                symbols.len(),
                grid,
            )
            .unwrap();

        assert!((info.scale_x - 1.0).abs() < 0.02);
        assert!((info.scale_y - 1.0).abs() < 0.02);

        let decoded = decode_optical_cells(
            &registered,
            symbols.len(),
            grid,
            None,
        )
        .unwrap();
        let logical =
            decode_optical_repetition(&decoded, repetition).unwrap();
        assert_eq!(bit_error_count(&bits, &logical), 0);
    }

    #[test]
    fn optical_scale_registration_tolerates_one_pixel_rounding_clip() {
        use signal_frontier::{
            decode_optical_cells, decode_optical_repetition,
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 0, 1,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let frame = render_optical_cells(&symbols, grid).unwrap();

        let mut pixels =
            Vec::with_capacity((frame.width - 1) * frame.height);
        for y in 0..frame.height {
            let start = y * frame.width + 1;
            pixels.extend_from_slice(
                &frame.pixels[start..start + frame.width - 1],
            );
        }
        let clipped = OpticalGrayFrame {
            width: frame.width - 1,
            height: frame.height,
            pixels,
        };

        let (registered, info) =
            register_optical_translation_scale(
                &clipped,
                symbols.len(),
                grid,
            )
            .unwrap();

        assert_eq!(info.source_origin_x, 0);
        assert_eq!(info.source_width, clipped.width);

        let decoded = decode_optical_cells(
            &registered,
            symbols.len(),
            grid,
            None,
        )
        .unwrap();
        let logical =
            decode_optical_repetition(&decoded, repetition).unwrap();
        assert_eq!(bit_error_count(&bits, &logical), 0);
    }

    #[test]
    fn optical_scale_registration_supports_bounded_axis_scaling() {
        use signal_frontier::{
            decode_optical_cells, decode_optical_repetition,
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            1, 1, 0, 1, 0, 1, 0, 0,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let frame = render_optical_cells(&symbols, grid).unwrap();

        let width = frame.width * 3 / 2;
        let height = frame.height * 2;
        let mut scaled = OpticalGrayFrame {
            width,
            height,
            pixels: vec![0.0; width * height],
        };
        for y in 0..height {
            let source_y = (y * frame.height / height)
                .min(frame.height - 1);
            for x in 0..width {
                let source_x = (x * frame.width / width)
                    .min(frame.width - 1);
                scaled.pixels[y * width + x] =
                    frame.pixels[source_y * frame.width + source_x];
            }
        }

        let source = embed_optical_frame(
            &scaled,
            scaled.width + 96,
            scaled.height + 88,
            41,
            33,
        );

        let (registered, info) =
            register_optical_translation_scale(
                &source,
                symbols.len(),
                grid,
            )
            .unwrap();

        assert!((info.scale_x - 1.5).abs() < 0.04);
        assert!((info.scale_y - 2.0).abs() < 0.04);

        let decoded = decode_optical_cells(
            &registered,
            symbols.len(),
            grid,
            None,
        )
        .unwrap();
        let logical =
            decode_optical_repetition(&decoded, repetition).unwrap();
        assert_eq!(bit_error_count(&bits, &logical), 0);
    }

    #[test]
    fn optical_y4m_replay_recovers_quarter_turn_camera_rotation() {
        use signal_frontier::{
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 0, 1,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let base = render_optical_cells(&symbols, grid).unwrap();
        let rotated = rotate_optical_frame_quarter_turns(&base, 1);
        let canvas = embed_optical_frame(
            &rotated,
            rotated.width + 120,
            rotated.height + 100,
            47,
            39,
        );
        let video = y4m_from_frames(
            canvas.width,
            canvas.height,
            &[canvas],
        );

        let decoded = replay_optical_y4m_registered_scaled(
            &video,
            symbols.len(),
            grid,
            repetition,
            0,
            1,
        )
        .unwrap();

        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn optical_scale_registration_recovers_two_x_shifted_grid() {
        use signal_frontier::{
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            1, 1, 0, 1, 0, 1, 0, 0,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let frame = render_optical_cells(&symbols, grid).unwrap();
        let scaled = resize_optical_nearest(&frame, 2, 1);
        let source = embed_optical_frame(
            &scaled,
            scaled.width + 127,
            scaled.height + 99,
            43,
            37,
        );

        let (registered, info) =
            register_optical_translation_scale(
                &source,
                symbols.len(),
                grid,
            )
            .unwrap();

        assert!((info.scale_x - 2.0).abs() < 0.02);
        assert!((info.scale_y - 2.0).abs() < 0.02);
        assert_eq!(info.source_origin_x, 43);
        assert_eq!(info.source_origin_y, 37);
        assert_eq!(registered, frame);
    }

    #[test]
    fn optical_scaled_pgm_replay_roundtrips_multiple_scales() {
        use signal_frontier::{
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 0, 1,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let frame = render_optical_cells(&symbols, grid).unwrap();

        let scaled_a = resize_optical_nearest(&frame, 2, 1);
        let scaled_b = resize_optical_nearest(&frame, 3, 2);
        let first = embed_optical_frame(
            &scaled_a,
            scaled_a.width + 100,
            scaled_a.height + 88,
            31,
            29,
        );
        let second = embed_optical_frame(
            &scaled_b,
            scaled_b.width + 120,
            scaled_b.height + 92,
            47,
            33,
        );

        let decoded = replay_optical_pgm_sequence_registered_scaled(
            &[pgm_from_frame(&first), pgm_from_frame(&second)],
            symbols.len(),
            grid,
            repetition,
        )
        .unwrap();

        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn optical_translation_registration_finds_shifted_grid() {
        use signal_frontier::{
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            1, 1, 0, 1, 0, 1, 0, 0,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let frame = render_optical_cells(&symbols, grid).unwrap();
        let source = embed_optical_frame(
            &frame,
            frame.width + 91,
            frame.height + 73,
            37,
            29,
        );

        let (registered, info) =
            register_optical_translation(&source, symbols.len(), grid)
                .unwrap();

        assert_eq!(info.source_origin_x, 37);
        assert_eq!(info.source_origin_y, 29);
        assert_eq!(registered, frame);
    }

    #[test]
    fn optical_registered_pgm_replay_roundtrips_shifted_frames() {
        use signal_frontier::{
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 0, 1,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let frame = render_optical_cells(&symbols, grid).unwrap();

        let first = embed_optical_frame(
            &frame,
            frame.width + 80,
            frame.height + 60,
            21,
            17,
        );
        let second = embed_optical_frame(
            &frame,
            frame.width + 112,
            frame.height + 84,
            53,
            31,
        );

        let decoded = replay_optical_pgm_sequence_registered(
            &[pgm_from_frame(&first), pgm_from_frame(&second)],
            symbols.len(),
            grid,
            repetition,
        )
        .unwrap();

        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn y4m_parser_extracts_luma_frames_and_metadata() {
        let frame = OpticalGrayFrame {
            width: 4,
            height: 2,
            pixels: vec![
                0.0, 0.1, 0.2, 0.3,
                0.4, 0.5, 0.6, 0.7,
            ],
        };
        let bytes = y4m_from_frames(4, 2, &[frame]);
        let video = parse_y4m_gray_video(&bytes).unwrap();

        assert_eq!(video.width, 4);
        assert_eq!(video.height, 2);
        assert_eq!(video.fps_numerator, 30);
        assert_eq!(video.fps_denominator, 1);
        assert_eq!(video.chroma, "420jpeg");
        assert_eq!(video.luma_frames.len(), 1);
        assert_eq!(video.luma_frames[0].len(), 8);
    }

    #[test]
    fn optical_y4m_replay_roundtrips_scaled_shifted_video_frames() {
        use signal_frontier::{
            encode_optical_repetition, render_optical_cells,
        };

        let bits = vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 0, 1,
        ];
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let base = render_optical_cells(&symbols, grid).unwrap();

        let scaled_a = resize_optical_nearest(&base, 2, 1);
        let scaled_b = resize_optical_nearest(&base, 3, 2);
        let canvas_width = scaled_a.width.max(scaled_b.width) + 140;
        let canvas_height = scaled_a.height.max(scaled_b.height) + 110;

        let first = embed_optical_frame(
            &scaled_a,
            canvas_width,
            canvas_height,
            37,
            31,
        );
        let second = embed_optical_frame(
            &scaled_b,
            canvas_width,
            canvas_height,
            71,
            53,
        );
        let video = y4m_from_frames(
            canvas_width,
            canvas_height,
            &[first, second],
        );

        let decoded = replay_optical_y4m_registered_scaled(
            &video,
            symbols.len(),
            grid,
            repetition,
            0,
            2,
        )
        .unwrap();

        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn y4m_replay_rejects_frame_window_past_end() {
        let frame = OpticalGrayFrame {
            width: 4,
            height: 2,
            pixels: vec![0.0; 8],
        };
        let bytes = y4m_from_frames(4, 2, &[frame]);

        let result = replay_optical_y4m_registered_scaled(
            &bytes,
            1,
            OpticalGridConfig::camera_baseline(),
            OpticalRepetitionConfig::robust_default(),
            1,
            1,
        );
        assert!(matches!(
            result,
            Err(ReplayError::VideoFrameWindowOutside),
        ));
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
