use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcousticFskConfig {
    pub sample_rate_hz: u32,
    pub bit_rate_bps: u32,
    pub zero_hz: f32,
    pub one_hz: f32,
    pub amplitude: f32,
}

impl AcousticFskConfig {
    pub fn near_ultrasonic_50bps() -> Self {
        Self {
            sample_rate_hz: 48_000,
            bit_rate_bps: 50,
            zero_hz: 18_500.0,
            one_hz: 19_500.0,
            amplitude: 0.65,
        }
    }

    pub fn validate(self) -> Result<(), SignalError> {
        if self.sample_rate_hz == 0 || self.bit_rate_bps == 0 {
            return Err(SignalError::InvalidConfig(
                "sample and bit rates must be non-zero",
            ));
        }
        if !self.sample_rate_hz.is_multiple_of(self.bit_rate_bps) {
            return Err(SignalError::InvalidConfig(
                "sample_rate_hz must be divisible by bit_rate_bps",
            ));
        }
        let nyquist = self.sample_rate_hz as f32 / 2.0;
        if !(0.0 < self.zero_hz && self.zero_hz < nyquist) {
            return Err(SignalError::InvalidConfig(
                "zero_hz must be below Nyquist",
            ));
        }
        if !(0.0 < self.one_hz && self.one_hz < nyquist) {
            return Err(SignalError::InvalidConfig(
                "one_hz must be below Nyquist",
            ));
        }
        if (self.zero_hz - self.one_hz).abs() < f32::EPSILON {
            return Err(SignalError::InvalidConfig(
                "FSK tones must be distinct",
            ));
        }
        if !(0.0 < self.amplitude && self.amplitude <= 1.0) {
            return Err(SignalError::InvalidConfig(
                "amplitude must be in (0, 1]",
            ));
        }
        Ok(())
    }

    pub fn samples_per_bit(self) -> Result<usize, SignalError> {
        self.validate()?;
        Ok((self.sample_rate_hz / self.bit_rate_bps) as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcousticChannel {
    pub gain: f32,
    pub white_noise_amplitude: f32,
    pub clip_level: f32,
}

impl AcousticChannel {
    pub fn mild_room() -> Self {
        Self {
            gain: 0.65,
            white_noise_amplitude: 0.06,
            clip_level: 0.95,
        }
    }

    pub fn validate(self) -> Result<(), SignalError> {
        if self.gain < 0.0 {
            return Err(SignalError::InvalidConfig(
                "channel gain must be non-negative",
            ));
        }
        if self.white_noise_amplitude < 0.0 {
            return Err(SignalError::InvalidConfig(
                "noise amplitude must be non-negative",
            ));
        }
        if !(0.0 < self.clip_level && self.clip_level <= 1.0) {
            return Err(SignalError::InvalidConfig(
                "clip level must be in (0, 1]",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodeResult {
    pub bits: Vec<u8>,
    pub symbol_confidence: Vec<f32>,
    pub minimum_confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalError {
    InvalidConfig(&'static str),
    InvalidBit(u8),
    MisalignedSamples,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcousticImpulseTap {
    pub delay_samples: usize,
    pub gain: f32,
}

pub fn apply_acoustic_impulse_response(
    samples: &[f32],
    taps: &[AcousticImpulseTap],
) -> Result<Vec<f32>, SignalError> {
    if taps.is_empty() {
        return Err(SignalError::InvalidConfig(
            "acoustic impulse response requires at least one tap",
        ));
    }
    if taps.iter().any(|tap| !tap.gain.is_finite()) {
        return Err(SignalError::InvalidConfig(
            "acoustic impulse response gain must be finite",
        ));
    }

    let mut output = vec![0.0_f32; samples.len()];
    for (index, slot) in output.iter_mut().enumerate() {
        let mut value = 0.0_f32;
        for tap in taps {
            if index >= tap.delay_samples {
                value += samples[index - tap.delay_samples] * tap.gain;
            }
        }
        *slot = value.clamp(-1.0, 1.0);
    }
    Ok(output)
}

/// Models receiver sampling-clock mismatch while preserving the court's fixed
/// nominal sample count. Positive ppm advances through source samples faster;
/// negative ppm advances more slowly. Linear interpolation keeps the
/// impairment deterministic and avoids nearest-neighbor artifacts.
pub fn apply_clock_drift_resampling(
    samples: &[f32],
    drift_ppm: i32,
) -> Result<Vec<f32>, SignalError> {
    if drift_ppm.unsigned_abs() > 20_000 {
        return Err(SignalError::InvalidConfig(
            "clock drift exceeds 20000 ppm research bound",
        ));
    }
    if samples.is_empty() || drift_ppm == 0 {
        return Ok(samples.to_vec());
    }

    let ratio = 1.0_f64 + f64::from(drift_ppm) / 1_000_000.0_f64;
    if ratio <= 0.0 {
        return Err(SignalError::InvalidConfig(
            "clock drift produces non-positive sampling ratio",
        ));
    }

    let last = samples.len() - 1;
    let mut output = Vec::with_capacity(samples.len());
    for out_index in 0..samples.len() {
        let source = out_index as f64 * ratio;
        if source >= last as f64 {
            output.push(samples[last]);
            continue;
        }

        let left = source.floor() as usize;
        let right = left + 1;
        let fraction = (source - left as f64) as f32;
        output.push(
            samples[left] * (1.0 - fraction)
                + samples[right] * fraction,
        );
    }
    Ok(output)
}

pub fn encode_fsk(
    bits: &[u8],
    config: AcousticFskConfig,
) -> Result<Vec<f32>, SignalError> {
    let samples_per_bit = config.samples_per_bit()?;
    let mut output = Vec::with_capacity(bits.len() * samples_per_bit);

    for &bit in bits {
        let frequency = match bit {
            0 => config.zero_hz,
            1 => config.one_hz,
            value => return Err(SignalError::InvalidBit(value)),
        };

        for sample_index in 0..samples_per_bit {
            let time =
                sample_index as f32 / config.sample_rate_hz as f32;
            output.push(
                config.amplitude * (TAU * frequency * time).sin(),
            );
        }
    }

    Ok(output)
}

pub fn apply_acoustic_channel(
    samples: &[f32],
    channel: AcousticChannel,
    seed: u64,
) -> Result<Vec<f32>, SignalError> {
    channel.validate()?;
    let mut rng = Lcg::new(seed);

    Ok(samples
        .iter()
        .map(|sample| {
            let unit_noise = rng.next_unit_signed();
            let noisy = sample * channel.gain
                + unit_noise * channel.white_noise_amplitude;
            noisy.clamp(-channel.clip_level, channel.clip_level)
        })
        .collect())
}

pub fn decode_fsk(
    samples: &[f32],
    config: AcousticFskConfig,
) -> Result<DecodeResult, SignalError> {
    let samples_per_bit = config.samples_per_bit()?;
    if !samples.len().is_multiple_of(samples_per_bit) {
        return Err(SignalError::MisalignedSamples);
    }

    let mut bits = Vec::with_capacity(samples.len() / samples_per_bit);
    let mut confidence = Vec::with_capacity(bits.capacity());

    for symbol in samples.chunks_exact(samples_per_bit) {
        let zero_energy = tone_energy(
            symbol,
            config.sample_rate_hz,
            config.zero_hz,
        );
        let one_energy = tone_energy(
            symbol,
            config.sample_rate_hz,
            config.one_hz,
        );

        let (bit, high, low) = if one_energy > zero_energy {
            (1, one_energy, zero_energy)
        } else {
            (0, zero_energy, one_energy)
        };

        let symbol_confidence = if high <= f32::EPSILON {
            0.0
        } else {
            ((high - low) / high).clamp(0.0, 1.0)
        };

        bits.push(bit);
        confidence.push(symbol_confidence);
    }

    let minimum_confidence = confidence
        .iter()
        .copied()
        .fold(1.0_f32, f32::min);

    Ok(DecodeResult {
        bits,
        symbol_confidence: confidence,
        minimum_confidence,
    })
}

pub fn bit_error_count(expected: &[u8], actual: &[u8]) -> usize {
    let shared = expected
        .iter()
        .zip(actual)
        .filter(|(left, right)| left != right)
        .count();
    shared + expected.len().abs_diff(actual.len())
}

fn tone_energy(
    samples: &[f32],
    sample_rate_hz: u32,
    frequency_hz: f32,
) -> f32 {
    let phase_step = TAU * frequency_hz / sample_rate_hz as f32;
    let mut in_phase = 0.0_f32;
    let mut quadrature = 0.0_f32;

    for (index, &sample) in samples.iter().enumerate() {
        let phase = phase_step * index as f32;
        in_phase += sample * phase.cos();
        quadrature += sample * phase.sin();
    }

    in_phase.mul_add(in_phase, quadrature * quadrature)
}

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    fn next_unit_signed(&mut self) -> f32 {
        let unit = self.next_u32() as f32 / u32::MAX as f32;
        unit * 2.0 - 1.0
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpticalRepetitionConfig {
    pub repeats_per_bit: usize,
}

impl OpticalRepetitionConfig {
    pub fn robust_default() -> Self {
        Self { repeats_per_bit: 5 }
    }

    pub fn validate(self) -> Result<(), SignalError> {
        if self.repeats_per_bit == 0 {
            return Err(SignalError::InvalidConfig(
                "optical repeats_per_bit must be non-zero",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpticalImpairment {
    pub drop_every: Option<usize>,
    pub flip_every: Option<usize>,
}

pub fn encode_optical_repetition(
    bits: &[u8],
    config: OpticalRepetitionConfig,
) -> Result<Vec<u8>, SignalError> {
    config.validate()?;
    let mut symbols = Vec::with_capacity(
        bits.len().saturating_mul(config.repeats_per_bit),
    );

    for &bit in bits {
        if bit > 1 {
            return Err(SignalError::InvalidBit(bit));
        }
        symbols.extend(std::iter::repeat_n(
            bit,
            config.repeats_per_bit,
        ));
    }

    Ok(symbols)
}

pub fn apply_optical_impairment(
    symbols: &[u8],
    impairment: OpticalImpairment,
) -> Vec<Option<u8>> {
    symbols
        .iter()
        .enumerate()
        .map(|(index, &bit)| {
            let position = index + 1;

            if impairment
                .drop_every
                .is_some_and(|every| every > 0 && position.is_multiple_of(every))
            {
                return None;
            }

            let flip = impairment
                .flip_every
                .is_some_and(|every| every > 0 && position.is_multiple_of(every));

            Some(if flip { bit ^ 1 } else { bit })
        })
        .collect()
}

pub fn decode_optical_repetition(
    symbols: &[Option<u8>],
    config: OpticalRepetitionConfig,
) -> Result<Vec<u8>, SignalError> {
    config.validate()?;
    if !symbols.len().is_multiple_of(config.repeats_per_bit) {
        return Err(SignalError::MisalignedSamples);
    }

    let mut decoded = Vec::with_capacity(
        symbols.len() / config.repeats_per_bit,
    );

    for group in symbols.chunks_exact(config.repeats_per_bit) {
        let mut zeros = 0_usize;
        let mut ones = 0_usize;

        for symbol in group.iter().flatten() {
            match *symbol {
                0 => zeros += 1,
                1 => ones += 1,
                value => return Err(SignalError::InvalidBit(value)),
            }
        }

        if zeros == 0 && ones == 0 {
            return Err(SignalError::MisalignedSamples);
        }

        decoded.push(u8::from(ones > zeros));
    }

    Ok(decoded)
}


#[derive(Debug, Clone, PartialEq)]
pub struct OpticalGrayFrame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
}

impl OpticalGrayFrame {
    fn validate(&self) -> Result<(), SignalError> {
        if self.width == 0 || self.height == 0 {
            return Err(SignalError::InvalidConfig(
                "optical frame dimensions must be non-zero",
            ));
        }
        if self.pixels.len() != self.width.saturating_mul(self.height) {
            return Err(SignalError::InvalidConfig(
                "optical frame pixel length does not match dimensions",
            ));
        }
        if self
            .pixels
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(SignalError::InvalidConfig(
                "optical frame contains non-finite pixels",
            ));
        }
        Ok(())
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<f32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels.get(y * self.width + x).copied()
    }

    fn sample_bilinear(&self, x: f32, y: f32) -> f32 {
        if x < 0.0
            || y < 0.0
            || x > (self.width - 1) as f32
            || y > (self.height - 1) as f32
        {
            return 0.0;
        }

        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let tx = x - x0 as f32;
        let ty = y - y0 as f32;

        let p00 = self.pixels[y0 * self.width + x0];
        let p10 = self.pixels[y0 * self.width + x1];
        let p01 = self.pixels[y1 * self.width + x0];
        let p11 = self.pixels[y1 * self.width + x1];

        let top = p00 * (1.0 - tx) + p10 * tx;
        let bottom = p01 * (1.0 - tx) + p11 * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpticalGridConfig {
    pub columns: usize,
    pub cell_pixels: usize,
    pub quiet_zone_cells: usize,
    pub zero_level: f32,
    pub one_level: f32,
    pub threshold: f32,
    pub decision_margin: f32,
}

impl OpticalGridConfig {
    pub fn camera_baseline() -> Self {
        Self {
            columns: 16,
            cell_pixels: 10,
            quiet_zone_cells: 2,
            zero_level: 0.18,
            one_level: 0.90,
            threshold: 0.50,
            decision_margin: 0.08,
        }
    }

    pub fn validate(self) -> Result<(), SignalError> {
        if self.columns == 0 || self.cell_pixels < 4 {
            return Err(SignalError::InvalidConfig(
                "optical grid requires columns and >=4 pixels per cell",
            ));
        }
        if self.quiet_zone_cells == 0 {
            return Err(SignalError::InvalidConfig(
                "optical grid requires a quiet zone",
            ));
        }
        if !self.zero_level.is_finite()
            || !self.one_level.is_finite()
            || !self.threshold.is_finite()
            || !self.decision_margin.is_finite()
        {
            return Err(SignalError::InvalidConfig(
                "optical levels must be finite",
            ));
        }
        if !(0.0..=1.0).contains(&self.zero_level)
            || !(0.0..=1.0).contains(&self.one_level)
            || !(0.0..=1.0).contains(&self.threshold)
            || self.zero_level >= self.threshold
            || self.threshold >= self.one_level
            || self.decision_margin < 0.0
            || self.decision_margin >= (self.one_level - self.zero_level) / 2.0
        {
            return Err(SignalError::InvalidConfig(
                "optical levels/threshold/margin are invalid",
            ));
        }
        Ok(())
    }

    fn dimensions_for(
        self,
        symbol_count: usize,
    ) -> Result<(usize, usize), SignalError> {
        self.validate()?;
        let rows = symbol_count.max(1).div_ceil(self.columns);
        let width_cells = self
            .columns
            .checked_add(self.quiet_zone_cells.saturating_mul(2))
            .ok_or(SignalError::InvalidConfig(
                "optical grid width overflow",
            ))?;
        let height_cells = rows
            .checked_add(self.quiet_zone_cells.saturating_mul(2))
            .ok_or(SignalError::InvalidConfig(
                "optical grid height overflow",
            ))?;
        let width = width_cells
            .checked_mul(self.cell_pixels)
            .ok_or(SignalError::InvalidConfig(
                "optical frame width overflow",
            ))?;
        let height = height_cells
            .checked_mul(self.cell_pixels)
            .ok_or(SignalError::InvalidConfig(
                "optical frame height overflow",
            ))?;
        Ok((width, height))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpticalPerspective {
    pub top_scale: f32,
    pub bottom_scale: f32,
    pub horizontal_shift_pixels: f32,
}

impl OpticalPerspective {
    pub fn mild_keystone() -> Self {
        Self {
            top_scale: 0.84,
            bottom_scale: 0.98,
            horizontal_shift_pixels: 5.0,
        }
    }

    pub fn validate(self) -> Result<(), SignalError> {
        if !self.top_scale.is_finite()
            || !self.bottom_scale.is_finite()
            || !self.horizontal_shift_pixels.is_finite()
            || self.top_scale <= 0.25
            || self.bottom_scale <= 0.25
            || self.top_scale > 1.25
            || self.bottom_scale > 1.25
        {
            return Err(SignalError::InvalidConfig(
                "optical perspective parameters are invalid",
            ));
        }
        Ok(())
    }

    fn row_scale(self, y_norm: f32) -> f32 {
        self.top_scale
            + (self.bottom_scale - self.top_scale) * y_norm
    }

    fn row_shift(self, y_norm: f32) -> f32 {
        self.horizontal_shift_pixels * (y_norm - 0.5)
    }

    fn project(
        self,
        width: usize,
        height: usize,
        x: f32,
        y: f32,
    ) -> (f32, f32) {
        let center = (width.saturating_sub(1)) as f32 / 2.0;
        let y_norm = if height <= 1 {
            0.0
        } else {
            y / (height - 1) as f32
        };
        let scale = self.row_scale(y_norm);
        let shift = self.row_shift(y_norm);
        (center + (x - center) * scale + shift, y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpticalPhotometric {
    pub exposure: f32,
    pub gamma: f32,
}

impl OpticalPhotometric {
    pub fn phone_camera_baseline() -> Self {
        Self {
            exposure: 0.88,
            gamma: 1.15,
        }
    }
}

pub fn render_optical_cells(
    symbols: &[u8],
    config: OpticalGridConfig,
) -> Result<OpticalGrayFrame, SignalError> {
    let (width, height) = config.dimensions_for(symbols.len())?;
    let background = (config.zero_level * 0.25).clamp(0.0, 1.0);
    let mut frame = OpticalGrayFrame {
        width,
        height,
        pixels: vec![background; width.saturating_mul(height)],
    };

    for (index, &symbol) in symbols.iter().enumerate() {
        if symbol > 1 {
            return Err(SignalError::InvalidBit(symbol));
        }

        let row = index / config.columns;
        let column = index % config.columns;
        let x0 = (config.quiet_zone_cells + column) * config.cell_pixels;
        let y0 = (config.quiet_zone_cells + row) * config.cell_pixels;
        let level = if symbol == 1 {
            config.one_level
        } else {
            config.zero_level
        };

        for y in y0..y0 + config.cell_pixels {
            let start = y * width + x0;
            frame.pixels[start..start + config.cell_pixels].fill(level);
        }
    }

    Ok(frame)
}

pub fn warp_optical_perspective(
    frame: &OpticalGrayFrame,
    perspective: OpticalPerspective,
) -> Result<OpticalGrayFrame, SignalError> {
    frame.validate()?;
    perspective.validate()?;

    let mut output = OpticalGrayFrame {
        width: frame.width,
        height: frame.height,
        pixels: vec![0.0; frame.pixels.len()],
    };
    let center = (frame.width.saturating_sub(1)) as f32 / 2.0;

    for y in 0..frame.height {
        let y_norm = if frame.height <= 1 {
            0.0
        } else {
            y as f32 / (frame.height - 1) as f32
        };
        let scale = perspective.row_scale(y_norm);
        let shift = perspective.row_shift(y_norm);

        for x in 0..frame.width {
            let source_x =
                center + (x as f32 - center - shift) / scale;
            output.pixels[y * frame.width + x] =
                frame.sample_bilinear(source_x, y as f32);
        }
    }

    Ok(output)
}

pub fn apply_optical_box_blur(
    frame: &OpticalGrayFrame,
    radius: usize,
) -> Result<OpticalGrayFrame, SignalError> {
    frame.validate()?;
    if radius == 0 {
        return Ok(frame.clone());
    }
    if radius > 16 {
        return Err(SignalError::InvalidConfig(
            "optical blur radius exceeds research bound",
        ));
    }

    let mut output = OpticalGrayFrame {
        width: frame.width,
        height: frame.height,
        pixels: vec![0.0; frame.pixels.len()],
    };

    for y in 0..frame.height {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius).min(frame.height - 1);
        for x in 0..frame.width {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius).min(frame.width - 1);
            let mut sum = 0.0_f32;
            let mut count = 0_usize;

            for sample_y in y0..=y1 {
                for sample_x in x0..=x1 {
                    sum += frame.pixels[sample_y * frame.width + sample_x];
                    count += 1;
                }
            }

            output.pixels[y * frame.width + x] = sum / count as f32;
        }
    }

    Ok(output)
}

pub fn apply_optical_photometric(
    frame: &OpticalGrayFrame,
    photometric: OpticalPhotometric,
) -> Result<OpticalGrayFrame, SignalError> {
    frame.validate()?;
    if !photometric.exposure.is_finite()
        || !photometric.gamma.is_finite()
        || photometric.exposure <= 0.0
        || photometric.gamma <= 0.0
        || photometric.exposure > 4.0
        || photometric.gamma > 4.0
    {
        return Err(SignalError::InvalidConfig(
            "optical exposure/gamma parameters are invalid",
        ));
    }

    let inverse_gamma = 1.0 / photometric.gamma;
    Ok(OpticalGrayFrame {
        width: frame.width,
        height: frame.height,
        pixels: frame
            .pixels
            .iter()
            .map(|value| {
                (value * photometric.exposure)
                    .clamp(0.0, 1.0)
                    .powf(inverse_gamma)
            })
            .collect(),
    })
}

pub fn decode_optical_cells(
    frame: &OpticalGrayFrame,
    symbol_count: usize,
    config: OpticalGridConfig,
    perspective: Option<OpticalPerspective>,
) -> Result<Vec<Option<u8>>, SignalError> {
    frame.validate()?;
    let (expected_width, expected_height) =
        config.dimensions_for(symbol_count)?;
    if frame.width != expected_width || frame.height != expected_height {
        return Err(SignalError::InvalidConfig(
            "optical frame dimensions do not match grid config",
        ));
    }
    if let Some(value) = perspective {
        value.validate()?;
    }

    let radius = (config.cell_pixels / 4).max(1) as isize;
    let mut decoded = Vec::with_capacity(symbol_count);

    for index in 0..symbol_count {
        let row = index / config.columns;
        let column = index % config.columns;
        let source_x =
            (config.quiet_zone_cells + column) as f32
                * config.cell_pixels as f32
                + config.cell_pixels as f32 / 2.0;
        let source_y =
            (config.quiet_zone_cells + row) as f32
                * config.cell_pixels as f32
                + config.cell_pixels as f32 / 2.0;

        let (center_x, center_y) = perspective.map_or(
            (source_x, source_y),
            |value| {
                value.project(
                    frame.width,
                    frame.height,
                    source_x,
                    source_y,
                )
            },
        );

        let mut sum = 0.0_f32;
        let mut count = 0_usize;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let x = center_x.round() as isize + dx;
                let y = center_y.round() as isize + dy;
                if x < 0
                    || y < 0
                    || x >= frame.width as isize
                    || y >= frame.height as isize
                {
                    continue;
                }
                sum += frame.pixels[y as usize * frame.width + x as usize];
                count += 1;
            }
        }

        if count == 0 {
            decoded.push(None);
            continue;
        }

        let mean = sum / count as f32;
        if (mean - config.threshold).abs() <= config.decision_margin {
            decoded.push(None);
        } else {
            decoded.push(Some(u8::from(mean > config.threshold)));
        }
    }

    Ok(decoded)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VibrationOokConfig {
    pub sample_rate_hz: u32,
    pub bit_duration_ms: u32,
    pub carrier_hz: f32,
    pub amplitude: f32,
    pub rms_threshold: f32,
}

impl VibrationOokConfig {
    pub fn surface_2_5bps() -> Self {
        Self {
            sample_rate_hz: 200,
            bit_duration_ms: 400,
            carrier_hz: 35.0,
            amplitude: 0.75,
            rms_threshold: 0.20,
        }
    }

    pub fn samples_per_bit(self) -> Result<usize, SignalError> {
        if self.sample_rate_hz == 0 || self.bit_duration_ms == 0 {
            return Err(SignalError::InvalidConfig(
                "vibration rate/duration must be non-zero",
            ));
        }
        let samples = u64::from(self.sample_rate_hz)
            .saturating_mul(u64::from(self.bit_duration_ms));
        if !samples.is_multiple_of(1000) {
            return Err(SignalError::InvalidConfig(
                "vibration symbol duration must align to sample rate",
            ));
        }

        let count = samples / 1000;
        if count == 0 {
            return Err(SignalError::InvalidConfig(
                "vibration symbol contains no samples",
            ));
        }

        let nyquist = self.sample_rate_hz as f32 / 2.0;
        if !(0.0 < self.carrier_hz && self.carrier_hz < nyquist) {
            return Err(SignalError::InvalidConfig(
                "vibration carrier must be below Nyquist",
            ));
        }
        if !(0.0 < self.amplitude && self.amplitude <= 1.0) {
            return Err(SignalError::InvalidConfig(
                "vibration amplitude must be in (0, 1]",
            ));
        }
        if self.rms_threshold <= 0.0 {
            return Err(SignalError::InvalidConfig(
                "vibration RMS threshold must be positive",
            ));
        }

        Ok(count as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MechanicalChannel {
    pub gain: f32,
    pub white_noise_amplitude: f32,
}

impl MechanicalChannel {
    pub fn shared_table() -> Self {
        Self {
            gain: 0.55,
            white_noise_amplitude: 0.05,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MechanicalImpulseTap {
    pub delay_samples: usize,
    pub gain: f32,
}

pub fn apply_mechanical_impulse_response(
    samples: &[f32],
    taps: &[MechanicalImpulseTap],
) -> Result<Vec<f32>, SignalError> {
    if taps.is_empty() {
        return Err(SignalError::InvalidConfig(
            "mechanical impulse response requires at least one tap",
        ));
    }
    if taps.iter().any(|tap| !tap.gain.is_finite()) {
        return Err(SignalError::InvalidConfig(
            "mechanical impulse response gain must be finite",
        ));
    }

    let mut output = vec![0.0_f32; samples.len()];
    for (index, slot) in output.iter_mut().enumerate() {
        let mut value = 0.0_f32;
        for tap in taps {
            if index >= tap.delay_samples {
                value += samples[index - tap.delay_samples] * tap.gain;
            }
        }
        *slot = value.clamp(-1.0, 1.0);
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VibrationMountProfile {
    pub signal_gain: f32,
    pub noise_scale: f32,
}

impl VibrationMountProfile {
    pub fn flat_table() -> Self {
        Self {
            signal_gain: 0.95,
            noise_scale: 1.00,
        }
    }

    pub fn edge_contact() -> Self {
        Self {
            signal_gain: 0.72,
            noise_scale: 1.20,
        }
    }

    pub fn handheld() -> Self {
        Self {
            signal_gain: 0.52,
            noise_scale: 1.80,
        }
    }

    pub fn validate(self) -> Result<(), SignalError> {
        if !self.signal_gain.is_finite() || !self.noise_scale.is_finite() {
            return Err(SignalError::InvalidConfig(
                "vibration mount profile parameters must be finite",
            ));
        }
        if !(0.0 < self.signal_gain && self.signal_gain <= 1.5) {
            return Err(SignalError::InvalidConfig(
                "vibration mount signal gain is invalid",
            ));
        }
        if !(0.0 < self.noise_scale && self.noise_scale <= 8.0) {
            return Err(SignalError::InvalidConfig(
                "vibration mount noise scale is invalid",
            ));
        }
        Ok(())
    }
}

pub fn apply_profiled_mechanical_channel(
    samples: &[f32],
    channel: MechanicalChannel,
    profile: VibrationMountProfile,
    seed: u64,
) -> Result<Vec<f32>, SignalError> {
    profile.validate()?;
    apply_mechanical_channel(
        samples,
        MechanicalChannel {
            gain: channel.gain * profile.signal_gain,
            white_noise_amplitude:
                channel.white_noise_amplitude * profile.noise_scale,
        },
        seed,
    )
}

pub fn encode_vibration_ook(
    bits: &[u8],
    config: VibrationOokConfig,
) -> Result<Vec<f32>, SignalError> {
    let samples_per_bit = config.samples_per_bit()?;
    let mut output = Vec::with_capacity(
        bits.len().saturating_mul(samples_per_bit),
    );

    for &bit in bits {
        if bit > 1 {
            return Err(SignalError::InvalidBit(bit));
        }

        for sample_index in 0..samples_per_bit {
            let time = sample_index as f32 / config.sample_rate_hz as f32;
            let carrier = (TAU * config.carrier_hz * time).sin();
            output.push(if bit == 1 {
                config.amplitude * carrier
            } else {
                0.0
            });
        }
    }

    Ok(output)
}

pub fn apply_mechanical_channel(
    samples: &[f32],
    channel: MechanicalChannel,
    seed: u64,
) -> Result<Vec<f32>, SignalError> {
    if channel.gain < 0.0 || channel.white_noise_amplitude < 0.0 {
        return Err(SignalError::InvalidConfig(
            "mechanical gain/noise must be non-negative",
        ));
    }

    let mut rng = Lcg::new(seed);
    Ok(samples
        .iter()
        .map(|sample| {
            sample * channel.gain
                + rng.next_unit_signed()
                    * channel.white_noise_amplitude
        })
        .collect())
}

pub fn decode_vibration_ook(
    samples: &[f32],
    config: VibrationOokConfig,
) -> Result<Vec<u8>, SignalError> {
    let samples_per_bit = config.samples_per_bit()?;
    if !samples.len().is_multiple_of(samples_per_bit) {
        return Err(SignalError::MisalignedSamples);
    }

    Ok(samples
        .chunks_exact(samples_per_bit)
        .map(|symbol| {
            let mean_square = symbol
                .iter()
                .map(|sample| sample * sample)
                .sum::<f32>()
                / symbol.len() as f32;
            let rms = mean_square.sqrt();
            u8::from(rms >= config.rms_threshold)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Vec<u8> {
        vec![
            1, 0, 1, 1, 0, 0, 1, 0,
            0, 1, 1, 0, 1, 0, 0, 1,
        ]
    }

    #[test]
    fn clean_near_ultrasonic_fsk_roundtrips_exactly() {
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        let bits = payload();
        let samples = encode_fsk(&bits, config).unwrap();
        let decoded = decode_fsk(&samples, config).unwrap();

        assert_eq!(decoded.bits, bits);
        assert_eq!(bit_error_count(&bits, &decoded.bits), 0);
        assert!(decoded.minimum_confidence > 0.95);
    }

    #[test]
    fn mild_room_noise_still_roundtrips_reference_payload() {
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        let bits = payload();
        let samples = encode_fsk(&bits, config).unwrap();
        let impaired = apply_acoustic_channel(
            &samples,
            AcousticChannel::mild_room(),
            42,
        )
        .unwrap();
        let decoded = decode_fsk(&impaired, config).unwrap();

        assert_eq!(bit_error_count(&bits, &decoded.bits), 0);
        assert!(decoded.minimum_confidence > 0.80);
    }

    #[test]
    fn near_ultrasonic_survives_multipath_and_clock_drift_baseline() {
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        let bits = payload();
        let samples = encode_fsk(&bits, config).unwrap();

        let multipath = apply_acoustic_impulse_response(
            &samples,
            &[
                AcousticImpulseTap {
                    delay_samples: 0,
                    gain: 0.72,
                },
                AcousticImpulseTap {
                    delay_samples: 7,
                    gain: 0.16,
                },
                AcousticImpulseTap {
                    delay_samples: 19,
                    gain: -0.07,
                },
            ],
        )
        .unwrap();

        let drifted =
            apply_clock_drift_resampling(&multipath, 80).unwrap();
        let impaired = apply_acoustic_channel(
            &drifted,
            AcousticChannel {
                gain: 0.90,
                white_noise_amplitude: 0.025,
                clip_level: 0.95,
            },
            0xA11CE,
        )
        .unwrap();

        let decoded = decode_fsk(&impaired, config).unwrap();
        assert_eq!(bit_error_count(&bits, &decoded.bits), 0);
        assert!(decoded.minimum_confidence > 0.45);
    }

    #[test]
    fn multipath_convolution_is_causal_and_deterministic() {
        let input = vec![1.0_f32, 0.0, 0.0, 0.0];
        let taps = [
            AcousticImpulseTap {
                delay_samples: 0,
                gain: 0.5,
            },
            AcousticImpulseTap {
                delay_samples: 2,
                gain: 0.25,
            },
        ];

        let output =
            apply_acoustic_impulse_response(&input, &taps).unwrap();
        assert_eq!(output, vec![0.5, 0.0, 0.25, 0.0]);
    }

    #[test]
    fn clock_drift_is_deterministic_and_bounded() {
        let input = (0..128)
            .map(|index| index as f32 / 127.0)
            .collect::<Vec<_>>();

        let first =
            apply_clock_drift_resampling(&input, 120).unwrap();
        let second =
            apply_clock_drift_resampling(&input, 120).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), input.len());
        assert_eq!(
            apply_clock_drift_resampling(&input, 20_001),
            Err(SignalError::InvalidConfig(
                "clock drift exceeds 20000 ppm research bound",
            )),
        );
    }

    #[test]
    fn channel_noise_is_deterministic_for_same_seed() {
        let input = vec![0.1_f32; 16];
        let channel = AcousticChannel::mild_room();

        let first =
            apply_acoustic_channel(&input, channel, 7).unwrap();
        let second =
            apply_acoustic_channel(&input, channel, 7).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn optical_repetition_survives_drops_and_sparse_flips() {
        let bits = payload();
        let config = OpticalRepetitionConfig::robust_default();
        let encoded = encode_optical_repetition(&bits, config).unwrap();
        let impaired = apply_optical_impairment(
            &encoded,
            OpticalImpairment {
                drop_every: Some(7),
                flip_every: Some(11),
            },
        );
        let decoded =
            decode_optical_repetition(&impaired, config).unwrap();

        assert_eq!(decoded, bits);
        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn optical_raster_roundtrips_under_camera_like_impairments() {
        let bits = payload();
        let repetition = OpticalRepetitionConfig::robust_default();
        let grid = OpticalGridConfig::camera_baseline();
        let perspective = OpticalPerspective::mild_keystone();

        let symbols =
            encode_optical_repetition(&bits, repetition).unwrap();
        let rendered = render_optical_cells(&symbols, grid).unwrap();
        let warped =
            warp_optical_perspective(&rendered, perspective).unwrap();
        let blurred = apply_optical_box_blur(&warped, 1).unwrap();
        let photographed = apply_optical_photometric(
            &blurred,
            OpticalPhotometric::phone_camera_baseline(),
        )
        .unwrap();
        let sampled = decode_optical_cells(
            &photographed,
            symbols.len(),
            grid,
            Some(perspective),
        )
        .unwrap();
        let decoded =
            decode_optical_repetition(&sampled, repetition).unwrap();

        assert_eq!(decoded, bits);
        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn optical_renderer_has_deterministic_grid_dimensions() {
        let grid = OpticalGridConfig::camera_baseline();
        let frame = render_optical_cells(&[1; 32], grid).unwrap();

        assert_eq!(
            frame.width,
            (grid.columns + grid.quiet_zone_cells * 2)
                * grid.cell_pixels,
        );
        assert_eq!(
            frame.height,
            (2 + grid.quiet_zone_cells * 2) * grid.cell_pixels,
        );
        assert_eq!(frame.pixels.len(), frame.width * frame.height);
    }

    #[test]
    fn optical_decoder_rejects_wrong_geometry_dimensions() {
        let grid = OpticalGridConfig::camera_baseline();
        let mut frame = render_optical_cells(&[1, 0, 1], grid).unwrap();
        frame.width -= 1;

        assert_eq!(
            decode_optical_cells(&frame, 3, grid, None),
            Err(SignalError::InvalidConfig(
                "optical frame pixel length does not match dimensions",
            )),
        );
    }

    #[test]
    fn vibration_roundtrips_with_resonance_and_flat_table_profile() {
        let bits = payload();
        let config = VibrationOokConfig::surface_2_5bps();
        let encoded = encode_vibration_ook(&bits, config).unwrap();
        let resonant = apply_mechanical_impulse_response(
            &encoded,
            &[
                MechanicalImpulseTap {
                    delay_samples: 0,
                    gain: 0.90,
                },
                MechanicalImpulseTap {
                    delay_samples: 3,
                    gain: 0.10,
                },
                MechanicalImpulseTap {
                    delay_samples: 8,
                    gain: -0.03,
                },
            ],
        )
        .unwrap();
        let impaired = apply_profiled_mechanical_channel(
            &resonant,
            MechanicalChannel {
                gain: 0.62,
                white_noise_amplitude: 0.025,
            },
            VibrationMountProfile::flat_table(),
            0x051B_A710,
        )
        .unwrap();
        let decoded =
            decode_vibration_ook(&impaired, config).unwrap();

        assert_eq!(decoded, bits);
        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn mechanical_impulse_response_is_causal_and_deterministic() {
        let input = vec![1.0_f32, 0.0, 0.0, 0.0, 0.0];
        let taps = [
            MechanicalImpulseTap {
                delay_samples: 0,
                gain: 0.6,
            },
            MechanicalImpulseTap {
                delay_samples: 2,
                gain: 0.2,
            },
        ];

        let first =
            apply_mechanical_impulse_response(&input, &taps).unwrap();
        let second =
            apply_mechanical_impulse_response(&input, &taps).unwrap();

        assert_eq!(first, second);
        assert_eq!(first, vec![0.6, 0.0, 0.2, 0.0, 0.0]);
    }

    #[test]
    fn mount_profiles_model_progressively_harder_contact() {
        let flat = VibrationMountProfile::flat_table();
        let edge = VibrationMountProfile::edge_contact();
        let hand = VibrationMountProfile::handheld();

        assert!(flat.signal_gain > edge.signal_gain);
        assert!(edge.signal_gain > hand.signal_gain);
        assert!(flat.noise_scale < edge.noise_scale);
        assert!(edge.noise_scale < hand.noise_scale);

        flat.validate().unwrap();
        edge.validate().unwrap();
        hand.validate().unwrap();
    }

    #[test]
    fn vibration_trace_roundtrips_under_shared_surface_noise() {
        let bits = payload();
        let config = VibrationOokConfig::surface_2_5bps();
        let encoded = encode_vibration_ook(&bits, config).unwrap();
        let impaired = apply_mechanical_channel(
            &encoded,
            MechanicalChannel::shared_table(),
            99,
        )
        .unwrap();
        let decoded =
            decode_vibration_ook(&impaired, config).unwrap();

        assert_eq!(decoded, bits);
        assert_eq!(bit_error_count(&bits, &decoded), 0);
    }

    #[test]
    fn invalid_bit_is_rejected() {
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        assert_eq!(
            encode_fsk(&[0, 2, 1], config),
            Err(SignalError::InvalidBit(2))
        );
    }

    #[test]
    fn misaligned_samples_are_rejected() {
        let config = AcousticFskConfig::near_ultrasonic_50bps();
        assert_eq!(
            decode_fsk(&[0.0; 10], config),
            Err(SignalError::MisalignedSamples)
        );
    }
}
