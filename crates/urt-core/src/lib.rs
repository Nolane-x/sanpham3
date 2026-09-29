//! Universal Reconstruction Transport (URT) v0.
//!
//! Conservative invariants:
//! - exact mode is byte-perfect or it fails;
//! - cache/delta wins report shared-state bytes separately;
//! - high-entropy input safely falls back to raw;
//! - decoding is deterministic and resource-bounded;
//! - streaming decode is the primary API.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};
use std::time::Duration;

pub const MAGIC: [u8; 4] = *b"URT0";
pub const FORMAT_VERSION: u8 = 1;
pub const FIXED_HEADER_BYTES: usize = 54;
const RLE_SCRATCH_BYTES: usize = 8 * 1024;

pub type Digest32 = [u8; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExactStrategy {
    Raw = 0,
    RunLength = 1,
    RepeatPattern = 2,
    CacheReference = 3,
    BaseDelta = 4,
}

impl ExactStrategy {
    fn from_u8(value: u8) -> Result<Self, UrtError> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::RunLength),
            2 => Ok(Self::RepeatPattern),
            3 => Ok(Self::CacheReference),
            4 => Ok(Self::BaseDelta),
            _ => Err(UrtError::InvalidFormat("unknown exact strategy")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub len: u32,
    pub byte: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExactRepresentation {
    Raw(Vec<u8>),
    RunLength(Vec<Run>),
    RepeatPattern {
        pattern: Vec<u8>,
        repeats: u64,
    },
    CacheReference,
    BaseDelta {
        base_digest: Digest32,
        prefix_len: u64,
        suffix_len: u64,
        middle: Vec<u8>,
    },
}

impl ExactRepresentation {
    pub fn strategy(&self) -> ExactStrategy {
        match self {
            Self::Raw(_) => ExactStrategy::Raw,
            Self::RunLength(_) => ExactStrategy::RunLength,
            Self::RepeatPattern { .. } => ExactStrategy::RepeatPattern,
            Self::CacheReference => ExactStrategy::CacheReference,
            Self::BaseDelta { .. } => ExactStrategy::BaseDelta,
        }
    }

    fn payload_bytes(&self) -> Vec<u8> {
        match self {
            Self::Raw(bytes) => bytes.clone(),
            Self::RunLength(runs) => {
                let mut out = Vec::with_capacity(runs.len().saturating_mul(5));
                for run in runs {
                    out.extend_from_slice(&run.len.to_le_bytes());
                    out.push(run.byte);
                }
                out
            }
            Self::RepeatPattern { pattern, repeats } => {
                let mut out = Vec::with_capacity(12 + pattern.len());
                out.extend_from_slice(&(pattern.len() as u32).to_le_bytes());
                out.extend_from_slice(&repeats.to_le_bytes());
                out.extend_from_slice(pattern);
                out
            }
            Self::CacheReference => Vec::new(),
            Self::BaseDelta {
                base_digest,
                prefix_len,
                suffix_len,
                middle,
            } => {
                let mut out = Vec::with_capacity(48 + middle.len());
                out.extend_from_slice(base_digest);
                out.extend_from_slice(&prefix_len.to_le_bytes());
                out.extend_from_slice(&suffix_len.to_le_bytes());
                out.extend_from_slice(middle);
                out
            }
        }
    }

    fn extra_decode_scratch_bytes(&self) -> usize {
        match self {
            Self::RunLength(_) => RLE_SCRATCH_BYTES,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactPacket {
    pub original_len: u64,
    pub digest: Digest32,
    pub representation: ExactRepresentation,
}

impl ExactPacket {
    pub fn strategy(&self) -> ExactStrategy {
        self.representation.strategy()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = self.representation.payload_bytes();
        let mut out = Vec::with_capacity(FIXED_HEADER_BYTES + payload.len());
        out.extend_from_slice(&MAGIC);
        out.push(FORMAT_VERSION);
        out.push(self.strategy() as u8);
        out.extend_from_slice(&self.original_len.to_le_bytes());
        out.extend_from_slice(&self.digest);
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self, UrtError> {
        if input.len() < FIXED_HEADER_BYTES {
            return Err(UrtError::InvalidFormat("packet shorter than URT header"));
        }

        let mut cursor = Cursor::new(input);
        if cursor.take(4)? != MAGIC {
            return Err(UrtError::InvalidFormat("bad URT magic"));
        }
        let version = cursor.u8()?;
        if version != FORMAT_VERSION {
            return Err(UrtError::UnsupportedVersion(version));
        }
        let strategy = ExactStrategy::from_u8(cursor.u8()?)?;
        let original_len = cursor.u64()?;
        let digest = cursor.array32()?;
        let payload_len_u64 = cursor.u64()?;
        let payload_len = usize::try_from(payload_len_u64)
            .map_err(|_| UrtError::InvalidFormat("payload length does not fit this platform"))?;
        let payload = cursor.take(payload_len)?;
        if cursor.remaining() != 0 {
            return Err(UrtError::InvalidFormat("trailing bytes after URT payload"));
        }

        let representation = match strategy {
            ExactStrategy::Raw => ExactRepresentation::Raw(payload.to_vec()),
            ExactStrategy::RunLength => {
                if payload.len() % 5 != 0 {
                    return Err(UrtError::InvalidFormat(
                        "RLE payload is not a sequence of 5-byte runs",
                    ));
                }
                let mut runs = Vec::with_capacity(payload.len() / 5);
                let mut run_cursor = Cursor::new(payload);
                while run_cursor.remaining() > 0 {
                    let len = run_cursor.u32()?;
                    if len == 0 {
                        return Err(UrtError::InvalidFormat("zero-length RLE run"));
                    }
                    runs.push(Run {
                        len,
                        byte: run_cursor.u8()?,
                    });
                }
                ExactRepresentation::RunLength(runs)
            }
            ExactStrategy::RepeatPattern => {
                let mut p = Cursor::new(payload);
                let pattern_len = p.u32()? as usize;
                let repeats = p.u64()?;
                if pattern_len == 0 || repeats < 2 {
                    return Err(UrtError::InvalidFormat("invalid repeat-pattern parameters"));
                }
                let pattern = p.take(pattern_len)?.to_vec();
                if p.remaining() != 0 {
                    return Err(UrtError::InvalidFormat("trailing repeat-pattern bytes"));
                }
                ExactRepresentation::RepeatPattern { pattern, repeats }
            }
            ExactStrategy::CacheReference => {
                if !payload.is_empty() {
                    return Err(UrtError::InvalidFormat(
                        "cache-reference payload must be empty",
                    ));
                }
                ExactRepresentation::CacheReference
            }
            ExactStrategy::BaseDelta => {
                let mut p = Cursor::new(payload);
                let base_digest = p.array32()?;
                let prefix_len = p.u64()?;
                let suffix_len = p.u64()?;
                let middle = p.take(p.remaining())?.to_vec();
                ExactRepresentation::BaseDelta {
                    base_digest,
                    prefix_len,
                    suffix_len,
                    middle,
                }
            }
        };

        Ok(Self {
            original_len,
            digest,
            representation,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeBudget {
    pub max_output_bytes: u64,
    pub max_decode_ops: u64,
    pub max_extra_working_bytes: usize,
}

impl DecodeBudget {
    pub fn permissive_for(output_bytes: u64) -> Self {
        Self {
            max_output_bytes: output_bytes,
            max_decode_ops: output_bytes.saturating_mul(4).saturating_add(1024),
            max_extra_working_bytes: RLE_SCRATCH_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactReport {
    pub strategy: ExactStrategy,
    pub original_bytes: u64,
    pub network_bytes: u64,
    pub shared_state_bytes: u64,
    pub saved_network_bytes: u64,
}

impl ExactReport {
    pub fn network_fraction_ppm(&self) -> u32 {
        if self.original_bytes == 0 {
            return 0;
        }
        ((u128::from(self.network_bytes) * 1_000_000_u128)
            / u128::from(self.original_bytes))
            .min(1_000_000) as u32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeResult {
    pub packet: ExactPacket,
    pub report: ExactReport,
}

pub trait ExactCache {
    fn get(&self, digest: &Digest32) -> Option<&[u8]>;
}

#[derive(Debug, Default)]
pub struct EmptyCache;

impl ExactCache for EmptyCache {
    fn get(&self, _digest: &Digest32) -> Option<&[u8]> {
        None
    }
}

impl ExactCache for HashMap<Digest32, Vec<u8>> {
    fn get(&self, digest: &Digest32) -> Option<&[u8]> {
        HashMap::get(self, digest).map(Vec::as_slice)
    }
}

/// Chooses the smallest currently implemented exact representation.
///
/// known_base means the receiver is known to possess exactly those bytes.
/// Cache/delta reports list that shared state separately instead of pretending
/// it is standalone compression.
pub fn encode_exact(input: &[u8], known_base: Option<&[u8]>) -> EncodeResult {
    let digest = sha256(input);
    let original_len = input.len() as u64;
    let mut representation = ExactRepresentation::Raw(input.to_vec());
    let mut best_payload_len = input.len();
    let mut shared_state_bytes = 0_u64;

    if let Some(runs) = encode_rle(input) {
        let payload_len = runs.len().saturating_mul(5);
        if payload_len < best_payload_len {
            best_payload_len = payload_len;
            representation = ExactRepresentation::RunLength(runs);
            shared_state_bytes = 0;
        }
    }

    if let Some((pattern, repeats)) = detect_repeat_pattern(input) {
        let payload_len = 12_usize.saturating_add(pattern.len());
        if payload_len < best_payload_len {
            best_payload_len = payload_len;
            representation = ExactRepresentation::RepeatPattern { pattern, repeats };
            shared_state_bytes = 0;
        }
    }

    if let Some(base) = known_base {
        let base_digest = sha256(base);
        if base == input {
            best_payload_len = 0;
            representation = ExactRepresentation::CacheReference;
            shared_state_bytes = base.len() as u64;
        } else {
            let (prefix_len, suffix_len, middle) = delta_against_base(input, base);
            let payload_len = 48_usize.saturating_add(middle.len());
            if payload_len < best_payload_len {
                best_payload_len = payload_len;
                representation = ExactRepresentation::BaseDelta {
                    base_digest,
                    prefix_len,
                    suffix_len,
                    middle,
                };
                shared_state_bytes = base.len() as u64;
            }
        }
    }

    let packet = ExactPacket {
        original_len,
        digest,
        representation,
    };
    let network_bytes = packet.to_bytes().len() as u64;
    let saved_network_bytes = original_len.saturating_sub(network_bytes);

    debug_assert_eq!(
        best_payload_len as u64 + FIXED_HEADER_BYTES as u64,
        network_bytes
    );

    let report = ExactReport {
        strategy: packet.strategy(),
        original_bytes: original_len,
        network_bytes,
        shared_state_bytes,
        saved_network_bytes,
    };

    EncodeResult { packet, report }
}

pub fn decode_exact<C: ExactCache>(
    wire: &[u8],
    cache: &C,
    budget: DecodeBudget,
) -> Result<Vec<u8>, UrtError> {
    let packet = ExactPacket::from_bytes(wire)?;
    if packet.original_len > budget.max_extra_working_bytes as u64 {
        return Err(UrtError::ResourceLimit(
            "materialized decode exceeds max_extra_working_bytes; use decode_exact_to_writer",
        ));
    }
    let capacity = usize::try_from(packet.original_len)
        .map_err(|_| UrtError::ResourceLimit("output does not fit address space"))?;
    let mut out = Vec::with_capacity(capacity);
    decode_packet_to_writer(&packet, cache, budget, &mut out)?;
    Ok(out)
}

pub fn decode_exact_to_writer<C: ExactCache, W: Write>(
    wire: &[u8],
    cache: &C,
    budget: DecodeBudget,
    writer: &mut W,
) -> Result<(), UrtError> {
    let packet = ExactPacket::from_bytes(wire)?;
    decode_packet_to_writer(&packet, cache, budget, writer)
}

pub fn sha256(bytes: &[u8]) -> Digest32 {
    Sha256::digest(bytes).into()
}

pub fn serialization_time(network_bytes: u64, bitrate_bps: u64) -> Option<Duration> {
    if bitrate_bps == 0 {
        return None;
    }
    let bits = u128::from(network_bytes).saturating_mul(8);
    let nanos = bits
        .saturating_mul(1_000_000_000)
        .div_ceil(u128::from(bitrate_bps));
    let secs = (nanos / 1_000_000_000).min(u128::from(u64::MAX)) as u64;
    Some(Duration::new(secs, (nanos % 1_000_000_000) as u32))
}

fn decode_packet_to_writer<C: ExactCache, W: Write>(
    packet: &ExactPacket,
    cache: &C,
    budget: DecodeBudget,
    writer: &mut W,
) -> Result<(), UrtError> {
    if packet.original_len > budget.max_output_bytes {
        return Err(UrtError::ResourceLimit("output exceeds max_output_bytes"));
    }
    if packet.representation.extra_decode_scratch_bytes() > budget.max_extra_working_bytes {
        return Err(UrtError::ResourceLimit(
            "representation exceeds max_extra_working_bytes",
        ));
    }

    let mut state = DecodeState {
        writer,
        hasher: Sha256::new(),
        written: 0,
        ops: 0,
        budget,
    };

    match &packet.representation {
        ExactRepresentation::Raw(bytes) => state.emit(bytes)?,
        ExactRepresentation::RunLength(runs) => {
            let mut scratch = [0_u8; RLE_SCRATCH_BYTES];
            for run in runs {
                scratch.fill(run.byte);
                let mut remaining = run.len as usize;
                while remaining > 0 {
                    let take = remaining.min(scratch.len());
                    state.emit(&scratch[..take])?;
                    remaining -= take;
                }
            }
        }
        ExactRepresentation::RepeatPattern { pattern, repeats } => {
            for _ in 0..*repeats {
                state.emit(pattern)?;
            }
        }
        ExactRepresentation::CacheReference => {
            let base = cache
                .get(&packet.digest)
                .ok_or(UrtError::MissingSharedState(packet.digest))?;
            state.emit(base)?;
        }
        ExactRepresentation::BaseDelta {
            base_digest,
            prefix_len,
            suffix_len,
            middle,
        } => {
            let base = cache
                .get(base_digest)
                .ok_or(UrtError::MissingSharedState(*base_digest))?;
            let prefix = usize::try_from(*prefix_len)
                .map_err(|_| UrtError::InvalidFormat("prefix length overflow"))?;
            let suffix = usize::try_from(*suffix_len)
                .map_err(|_| UrtError::InvalidFormat("suffix length overflow"))?;
            if prefix.saturating_add(suffix) > base.len() {
                return Err(UrtError::InvalidFormat(
                    "delta prefix/suffix exceed base object",
                ));
            }

            state.emit(&base[..prefix])?;
            state.emit(middle)?;
            if suffix > 0 {
                state.emit(&base[base.len() - suffix..])?;
            }
        }
    }

    if state.written != packet.original_len {
        return Err(UrtError::LengthMismatch {
            expected: packet.original_len,
            actual: state.written,
        });
    }

    let actual: Digest32 = state.hasher.finalize().into();
    if actual != packet.digest {
        return Err(UrtError::IntegrityMismatch);
    }
    Ok(())
}

struct DecodeState<'a, W: Write> {
    writer: &'a mut W,
    hasher: Sha256,
    written: u64,
    ops: u64,
    budget: DecodeBudget,
}

impl<W: Write> DecodeState<'_, W> {
    fn emit(&mut self, bytes: &[u8]) -> Result<(), UrtError> {
        let len = bytes.len() as u64;
        let next_written = self
            .written
            .checked_add(len)
            .ok_or(UrtError::ResourceLimit("output byte counter overflow"))?;
        if next_written > self.budget.max_output_bytes {
            return Err(UrtError::ResourceLimit("output exceeds max_output_bytes"));
        }
        let next_ops = self
            .ops
            .checked_add(len.max(1))
            .ok_or(UrtError::ResourceLimit("decode operation counter overflow"))?;
        if next_ops > self.budget.max_decode_ops {
            return Err(UrtError::ResourceLimit("decode exceeds max_decode_ops"));
        }

        self.writer.write_all(bytes)?;
        self.hasher.update(bytes);
        self.written = next_written;
        self.ops = next_ops;
        Ok(())
    }
}

fn encode_rle(input: &[u8]) -> Option<Vec<Run>> {
    let (&first, rest) = input.split_first()?;
    let mut runs = Vec::new();
    let mut current = first;
    let mut len = 1_u32;

    for &byte in rest {
        if byte == current && len < u32::MAX {
            len += 1;
        } else {
            runs.push(Run { len, byte: current });
            current = byte;
            len = 1;
        }
    }
    runs.push(Run { len, byte: current });

    (runs.len().saturating_mul(5) < input.len()).then_some(runs)
}

fn detect_repeat_pattern(input: &[u8]) -> Option<(Vec<u8>, u64)> {
    if input.len() < 2 {
        return None;
    }

    let mut prefix = vec![0_usize; input.len()];
    for i in 1..input.len() {
        let mut j = prefix[i - 1];
        while j > 0 && input[i] != input[j] {
            j = prefix[j - 1];
        }
        if input[i] == input[j] {
            j += 1;
        }
        prefix[i] = j;
    }

    let period = input.len() - prefix[input.len() - 1];
    if period == input.len() || !input.len().is_multiple_of(period) {
        return None;
    }
    let repeats = input.len() / period;
    if repeats < 2 || period > u32::MAX as usize {
        return None;
    }
    Some((input[..period].to_vec(), repeats as u64))
}

fn delta_against_base(input: &[u8], base: &[u8]) -> (u64, u64, Vec<u8>) {
    let shared = input.len().min(base.len());
    let mut prefix = 0_usize;
    while prefix < shared && input[prefix] == base[prefix] {
        prefix += 1;
    }

    let mut suffix = 0_usize;
    while suffix < shared.saturating_sub(prefix)
        && input[input.len() - 1 - suffix] == base[base.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let middle_end = input.len().saturating_sub(suffix);
    (
        prefix as u64,
        suffix as u64,
        input[prefix..middle_end].to_vec(),
    )
}

#[derive(Debug)]
pub enum UrtError {
    InvalidFormat(&'static str),
    UnsupportedVersion(u8),
    MissingSharedState(Digest32),
    ResourceLimit(&'static str),
    LengthMismatch { expected: u64, actual: u64 },
    IntegrityMismatch,
    Io(io::Error),
}

impl fmt::Display for UrtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(message) => write!(f, "invalid URT packet: {message}"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported URT version {version}"),
            Self::MissingSharedState(_) => write!(f, "required shared-state object is unavailable"),
            Self::ResourceLimit(message) => write!(f, "URT resource limit: {message}"),
            Self::LengthMismatch { expected, actual } => {
                write!(f, "URT output length mismatch: expected {expected}, got {actual}")
            }
            Self::IntegrityMismatch => write!(f, "URT exact reconstruction hash mismatch"),
            Self::Io(error) => write!(f, "URT output error: {error}"),
        }
    }
}

impl std::error::Error for UrtError {}

impl From<io::Error> for UrtError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

struct Cursor<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.pos)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], UrtError> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(UrtError::InvalidFormat("cursor overflow"))?;
        if end > self.input.len() {
            return Err(UrtError::InvalidFormat("truncated URT packet"));
        }
        let out = &self.input[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, UrtError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, UrtError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| UrtError::InvalidFormat("truncated u32"))?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, UrtError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| UrtError::InvalidFormat("truncated u64"))?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn array32(&mut self) -> Result<Digest32, UrtError> {
        self.take(32)?
            .try_into()
            .map_err(|_| UrtError::InvalidFormat("truncated digest"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(input: &[u8], base: Option<&[u8]>) -> EncodeResult {
        let encoded = encode_exact(input, base);
        let wire = encoded.packet.to_bytes();
        let mut cache = HashMap::<Digest32, Vec<u8>>::new();
        if let Some(base) = base {
            cache.insert(sha256(base), base.to_vec());
        }
        let budget = DecodeBudget {
            max_output_bytes: input.len() as u64,
            max_decode_ops: (input.len() as u64).saturating_mul(2).saturating_add(1024),
            max_extra_working_bytes: input.len().max(RLE_SCRATCH_BYTES),
        };
        let decoded = decode_exact(&wire, &cache, budget).unwrap();
        assert_eq!(decoded, input);
        assert_eq!(sha256(&decoded), sha256(input));
        encoded
    }

    #[test]
    fn high_entropy_like_data_falls_back_to_raw_without_false_magic() {
        let mut state = 0x1234_5678_9abc_def0_u64;
        let mut input = vec![0_u8; 64 * 1024];
        for byte in &mut input {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }

        let encoded = round_trip(&input, None);
        assert_eq!(encoded.report.strategy, ExactStrategy::Raw);
        assert!(encoded.report.network_bytes >= encoded.report.original_bytes);
        assert_eq!(encoded.report.shared_state_bytes, 0);
    }

    #[test]
    fn repeated_structure_collapses_to_small_exact_program() {
        let input = b"ABCD".repeat(256 * 1024);
        let encoded = round_trip(&input, None);

        assert_eq!(encoded.report.strategy, ExactStrategy::RepeatPattern);
        assert_eq!(encoded.report.shared_state_bytes, 0);
        assert!(encoded.report.network_bytes < 128);
    }

    #[test]
    fn cache_hit_reports_shared_state_instead_of_fake_standalone_ratio() {
        let input = b"already present at receiver".repeat(4096);
        let encoded = round_trip(&input, Some(&input));

        assert_eq!(encoded.report.strategy, ExactStrategy::CacheReference);
        assert_eq!(encoded.report.shared_state_bytes, input.len() as u64);
        assert_eq!(encoded.report.network_bytes, FIXED_HEADER_BYTES as u64);
    }

    #[test]
    fn small_edit_uses_exact_base_delta_and_reconstructs_identically() {
        let mut base = Vec::with_capacity(256 * 1024);
        for i in 0..256 * 1024 {
            base.push((i % 251) as u8);
        }
        let mut changed = base.clone();
        changed[120_000..120_032].copy_from_slice(&[0xA5; 32]);

        let encoded = round_trip(&changed, Some(&base));
        assert_eq!(encoded.report.strategy, ExactStrategy::BaseDelta);
        assert_eq!(encoded.report.shared_state_bytes, base.len() as u64);
        assert!(encoded.report.network_bytes < 256);
    }

    #[test]
    fn tampering_is_detected_by_exact_hash_contract() {
        let input = b"exact exact exact exact exact".repeat(100);
        let mut wire = encode_exact(&input, None).packet.to_bytes();
        let last = wire.len() - 1;
        wire[last] ^= 1;

        let cache = EmptyCache;
        let mut sink = Vec::new();
        let result = decode_exact_to_writer(
            &wire,
            &cache,
            DecodeBudget::permissive_for(input.len() as u64),
            &mut sink,
        );
        assert!(matches!(result, Err(UrtError::IntegrityMismatch)));
    }

    #[test]
    fn decode_budget_blocks_expansion_before_unbounded_output() {
        let input = vec![7_u8; 128 * 1024];
        let wire = encode_exact(&input, None).packet.to_bytes();
        let cache = EmptyCache;
        let mut sink = Vec::new();
        let result = decode_exact_to_writer(
            &wire,
            &cache,
            DecodeBudget {
                max_output_bytes: 1024,
                max_decode_ops: 4096,
                max_extra_working_bytes: RLE_SCRATCH_BYTES,
            },
            &mut sink,
        );
        assert!(matches!(result, Err(UrtError::ResourceLimit(_))));
        assert!(sink.is_empty());
    }

    #[test]
    fn ten_bps_projection_makes_network_byte_reduction_visible() {
        let input = vec![b'Z'; 1024 * 1024];
        let report = encode_exact(&input, None).report;
        let raw = serialization_time(input.len() as u64, 10).unwrap();
        let urt = serialization_time(report.network_bytes, 10).unwrap();

        assert!(urt < raw);
        assert!(raw > Duration::from_secs(800_000));
        assert!(urt < Duration::from_secs(120));
    }
}
