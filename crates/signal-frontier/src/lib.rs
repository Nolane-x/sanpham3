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
