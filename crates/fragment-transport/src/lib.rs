use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

type HmacSha256 = Hmac<Sha256>;

pub const MAGIC: [u8; 4] = *b"SP3F";
pub const VERSION: u8 = 1;
pub const TAG_BYTES: usize = 16;
pub const HEADER_BYTES: usize = 4 + 1 + 16 + 8 + 8 + 32 + 4;
pub const MIN_WIRE_BYTES: usize = HEADER_BYTES + TAG_BYTES;

pub type Digest32 = [u8; 32];
pub type TransferId = [u8; 16];

pub struct FragmentKey([u8; 32]);

impl FragmentKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for FragmentKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentEnvelope {
    pub transfer_id: TransferId,
    pub offset: u64,
    pub total_len: u64,
    pub whole_digest: Digest32,
    pub payload: Vec<u8>,
}

impl FragmentEnvelope {
    pub fn end_offset(&self) -> Result<u64, FragmentError> {
        self.offset
            .checked_add(self.payload.len() as u64)
            .ok_or(FragmentError::RangeOverflow)
    }

    pub fn seal(&self, key: &FragmentKey) -> Result<Vec<u8>, FragmentError> {
        validate_envelope(self)?;

        let payload_len = u32::try_from(self.payload.len())
            .map_err(|_| FragmentError::PayloadTooLarge)?;
        let mut wire =
            Vec::with_capacity(HEADER_BYTES + self.payload.len() + TAG_BYTES);

        wire.extend_from_slice(&MAGIC);
        wire.push(VERSION);
        wire.extend_from_slice(&self.transfer_id);
        wire.extend_from_slice(&self.offset.to_be_bytes());
        wire.extend_from_slice(&self.total_len.to_be_bytes());
        wire.extend_from_slice(&self.whole_digest);
        wire.extend_from_slice(&payload_len.to_be_bytes());
        wire.extend_from_slice(&self.payload);

        let tag = authentication_tag(&wire, key);
        wire.extend_from_slice(&tag);
        Ok(wire)
    }

    pub fn open(wire: &[u8], key: &FragmentKey) -> Result<Self, FragmentError> {
        if wire.len() < MIN_WIRE_BYTES {
            return Err(FragmentError::Truncated);
        }

        let authenticated_len = wire.len() - TAG_BYTES;
        let provided_tag = &wire[authenticated_len..];
        let expected_tag = authentication_tag(&wire[..authenticated_len], key);
        if !constant_time_eq(provided_tag, &expected_tag) {
            return Err(FragmentError::AuthenticationFailed);
        }

        let mut cursor = Cursor::new(&wire[..authenticated_len]);
        if cursor.take(4)? != MAGIC {
            return Err(FragmentError::WrongMagic);
        }
        let version = cursor.u8()?;
        if version != VERSION {
            return Err(FragmentError::WrongVersion(version));
        }

        let transfer_id = cursor.array16()?;
        let offset = cursor.u64()?;
        let total_len = cursor.u64()?;
        let whole_digest = cursor.array32()?;
        let payload_len = cursor.u32()? as usize;
        let payload = cursor.take(payload_len)?.to_vec();

        if cursor.remaining() != 0 {
            return Err(FragmentError::TrailingBytes);
        }

        let envelope = Self {
            transfer_id,
            offset,
            total_len,
            whole_digest,
            payload,
        };
        validate_envelope(&envelope)?;
        Ok(envelope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferDescriptor {
    pub transfer_id: TransferId,
    pub total_len: u64,
    pub whole_digest: Digest32,
}

impl TransferDescriptor {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let digest = sha256(bytes);
        let mut transfer_id = [0_u8; 16];
        transfer_id.copy_from_slice(&digest[..16]);
        Self {
            transfer_id,
            total_len: bytes.len() as u64,
            whole_digest: digest,
        }
    }
}

pub fn fragment_for_wire_budget(
    bytes: &[u8],
    wire_budget: usize,
    key: &FragmentKey,
) -> Result<Vec<Vec<u8>>, FragmentError> {
    if wire_budget <= MIN_WIRE_BYTES {
        return Err(FragmentError::WireBudgetTooSmall {
            minimum: MIN_WIRE_BYTES + 1,
            got: wire_budget,
        });
    }

    let payload_budget = wire_budget - MIN_WIRE_BYTES;
    let descriptor = TransferDescriptor::from_bytes(bytes);
    let mut wires = Vec::new();

    if bytes.is_empty() {
        let envelope = FragmentEnvelope {
            transfer_id: descriptor.transfer_id,
            offset: 0,
            total_len: 0,
            whole_digest: descriptor.whole_digest,
            payload: Vec::new(),
        };
        wires.push(envelope.seal(key)?);
        return Ok(wires);
    }

    for (index, payload) in bytes.chunks(payload_budget).enumerate() {
        let offset = (index as u64)
            .checked_mul(payload_budget as u64)
            .ok_or(FragmentError::RangeOverflow)?;
        let envelope = FragmentEnvelope {
            transfer_id: descriptor.transfer_id,
            offset,
            total_len: descriptor.total_len,
            whole_digest: descriptor.whole_digest,
            payload: payload.to_vec(),
        };
        wires.push(envelope.seal(key)?);
    }

    Ok(wires)
}

pub struct FragmentAssembler {
    max_total_len: u64,
    descriptor: Option<TransferDescriptor>,
    fragments: BTreeMap<u64, Vec<u8>>,
}

impl FragmentAssembler {
    pub fn new(max_total_len: u64) -> Self {
        Self {
            max_total_len,
            descriptor: None,
            fragments: BTreeMap::new(),
        }
    }

    pub fn accept_wire(
        &mut self,
        wire: &[u8],
        key: &FragmentKey,
    ) -> Result<AcceptOutcome, FragmentError> {
        let envelope = FragmentEnvelope::open(wire, key)?;
        self.accept(envelope)
    }

    pub fn accept(
        &mut self,
        envelope: FragmentEnvelope,
    ) -> Result<AcceptOutcome, FragmentError> {
        validate_envelope(&envelope)?;
        if envelope.total_len > self.max_total_len {
            return Err(FragmentError::ResourceLimit);
        }

        let descriptor = TransferDescriptor {
            transfer_id: envelope.transfer_id,
            total_len: envelope.total_len,
            whole_digest: envelope.whole_digest,
        };

        match &self.descriptor {
            None => self.descriptor = Some(descriptor),
            Some(existing) if existing == &descriptor => {}
            Some(_) => return Err(FragmentError::TransferMismatch),
        }

        if let Some(existing) = self.fragments.get(&envelope.offset) {
            return if existing == &envelope.payload {
                Ok(AcceptOutcome::Duplicate)
            } else {
                Err(FragmentError::ConflictingDuplicate)
            };
        }

        let new_start = envelope.offset;
        let new_end = envelope.end_offset()?;
        for (&start, payload) in &self.fragments {
            let end = start
                .checked_add(payload.len() as u64)
                .ok_or(FragmentError::RangeOverflow)?;
            if new_start < end && start < new_end {
                return Err(FragmentError::OverlappingRange);
            }
        }

        self.fragments.insert(envelope.offset, envelope.payload);
        Ok(AcceptOutcome::Accepted)
    }

    pub fn received_payload_bytes(&self) -> u64 {
        self.fragments
            .values()
            .map(|bytes| bytes.len() as u64)
            .sum()
    }

    pub fn fragment_count(&self) -> usize {
        self.fragments.len()
    }

    pub fn missing_ranges(&self) -> Vec<(u64, u64)> {
        let Some(descriptor) = &self.descriptor else {
            return Vec::new();
        };

        let mut ranges = Vec::new();
        let mut cursor = 0_u64;
        for (&offset, payload) in &self.fragments {
            if offset > cursor {
                ranges.push((cursor, offset));
            }
            cursor = cursor.max(offset.saturating_add(payload.len() as u64));
        }
        if cursor < descriptor.total_len {
            ranges.push((cursor, descriptor.total_len));
        }
        ranges
    }

    pub fn is_complete(&self) -> bool {
        self.descriptor.is_some() && self.missing_ranges().is_empty()
    }

    pub fn reconstruct(&self) -> Result<Vec<u8>, FragmentError> {
        let descriptor = self
            .descriptor
            .as_ref()
            .ok_or(FragmentError::Incomplete)?;

        if !self.is_complete() {
            return Err(FragmentError::Incomplete);
        }

        let capacity = usize::try_from(descriptor.total_len)
            .map_err(|_| FragmentError::ResourceLimit)?;
        let mut out = Vec::with_capacity(capacity);

        for (&offset, payload) in &self.fragments {
            if offset != out.len() as u64 {
                return Err(FragmentError::Incomplete);
            }
            out.extend_from_slice(payload);
        }

        if out.len() as u64 != descriptor.total_len {
            return Err(FragmentError::Incomplete);
        }
        if sha256(&out) != descriptor.whole_digest {
            return Err(FragmentError::WholeDigestMismatch);
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptOutcome {
    Accepted,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentError {
    Truncated,
    WrongMagic,
    WrongVersion(u8),
    AuthenticationFailed,
    PayloadTooLarge,
    RangeOverflow,
    RangeOutsideTransfer,
    EmptyPayloadForNonEmptyTransfer,
    WireBudgetTooSmall { minimum: usize, got: usize },
    TransferMismatch,
    ConflictingDuplicate,
    OverlappingRange,
    ResourceLimit,
    Incomplete,
    WholeDigestMismatch,
    TrailingBytes,
}

impl fmt::Display for FragmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "fragment envelope is truncated"),
            Self::WrongMagic => write!(f, "fragment envelope has wrong magic"),
            Self::WrongVersion(version) => {
                write!(f, "unsupported fragment envelope version {version}")
            }
            Self::AuthenticationFailed => {
                write!(f, "fragment envelope authentication failed")
            }
            Self::PayloadTooLarge => write!(f, "fragment payload is too large"),
            Self::RangeOverflow => write!(f, "fragment range overflow"),
            Self::RangeOutsideTransfer => {
                write!(f, "fragment range exceeds transfer length")
            }
            Self::EmptyPayloadForNonEmptyTransfer => {
                write!(f, "non-empty transfer contains empty fragment")
            }
            Self::WireBudgetTooSmall { minimum, got } => {
                write!(f, "wire budget {got} is below minimum {minimum}")
            }
            Self::TransferMismatch => {
                write!(f, "fragment belongs to a different transfer")
            }
            Self::ConflictingDuplicate => {
                write!(f, "duplicate offset contains different payload")
            }
            Self::OverlappingRange => write!(f, "fragment ranges overlap"),
            Self::ResourceLimit => write!(f, "fragment transfer exceeds resource limit"),
            Self::Incomplete => write!(f, "fragment transfer is incomplete"),
            Self::WholeDigestMismatch => {
                write!(f, "reconstructed transfer digest mismatch")
            }
            Self::TrailingBytes => write!(f, "fragment envelope contains trailing bytes"),
        }
    }
}

impl std::error::Error for FragmentError {}

pub fn sha256(bytes: &[u8]) -> Digest32 {
    Sha256::digest(bytes).into()
}

fn validate_envelope(envelope: &FragmentEnvelope) -> Result<(), FragmentError> {
    if envelope.total_len == 0 {
        if envelope.offset != 0 || !envelope.payload.is_empty() {
            return Err(FragmentError::RangeOutsideTransfer);
        }
        return Ok(());
    }

    if envelope.payload.is_empty() {
        return Err(FragmentError::EmptyPayloadForNonEmptyTransfer);
    }

    let end = envelope.end_offset()?;
    if envelope.offset >= envelope.total_len || end > envelope.total_len {
        return Err(FragmentError::RangeOutsideTransfer);
    }

    Ok(())
}

fn authentication_tag(bytes: &[u8], key: &FragmentKey) -> [u8; TAG_BYTES] {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(bytes);
    let digest = mac.finalize().into_bytes();
    let mut tag = [0_u8; TAG_BYTES];
    tag.copy_from_slice(&digest[..TAG_BYTES]);
    tag
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (&a, &b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], FragmentError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(FragmentError::Truncated)?;
        if end > self.bytes.len() {
            return Err(FragmentError::Truncated);
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, FragmentError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, FragmentError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| FragmentError::Truncated)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, FragmentError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| FragmentError::Truncated)?,
        ))
    }

    fn array16(&mut self) -> Result<[u8; 16], FragmentError> {
        self.take(16)?
            .try_into()
            .map_err(|_| FragmentError::Truncated)
    }

    fn array32(&mut self) -> Result<[u8; 32], FragmentError> {
        self.take(32)?
            .try_into()
            .map_err(|_| FragmentError::Truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> FragmentKey {
        FragmentKey::new([0x47; 32])
    }

    #[test]
    fn exact_roundtrip_accepts_shuffle_and_duplicates() {
        let input = (0..64 * 1024)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let key = key();
        let mut wires = fragment_for_wire_budget(&input, 512, &key).unwrap();

        wires.reverse();
        let duplicate = wires[3].clone();
        wires.insert(5, duplicate);

        let mut assembler = FragmentAssembler::new(128 * 1024);
        let mut duplicates = 0;
        for wire in wires {
            if assembler.accept_wire(&wire, &key).unwrap()
                == AcceptOutcome::Duplicate
            {
                duplicates += 1;
            }
        }

        assert_eq!(duplicates, 1);
        assert!(assembler.is_complete());
        assert_eq!(assembler.reconstruct().unwrap(), input);
    }

    #[test]
    fn tampering_and_wrong_key_are_rejected() {
        let input = b"authenticated fragment payload".repeat(100);
        let key = key();
        let mut wires = fragment_for_wire_budget(&input, 256, &key).unwrap();

        let last = wires[0].len() - 1;
        wires[0][last] ^= 1;
        assert_eq!(
            FragmentEnvelope::open(&wires[0], &key).unwrap_err(),
            FragmentError::AuthenticationFailed,
        );

        let other = FragmentKey::new([0x99; 32]);
        assert_eq!(
            FragmentEnvelope::open(&wires[1], &other).unwrap_err(),
            FragmentError::AuthenticationFailed,
        );
    }

    #[test]
    fn missing_ranges_report_exact_gaps() {
        let input = vec![0xAB; 4_000];
        let key = key();
        let wires = fragment_for_wire_budget(&input, 400, &key).unwrap();
        assert!(wires.len() > 3);

        let mut assembler = FragmentAssembler::new(8_000);
        assembler.accept_wire(&wires[0], &key).unwrap();
        assembler.accept_wire(&wires[2], &key).unwrap();

        let first = FragmentEnvelope::open(&wires[0], &key).unwrap();
        let second = FragmentEnvelope::open(&wires[1], &key).unwrap();
        let third = FragmentEnvelope::open(&wires[2], &key).unwrap();

        assert_eq!(
            assembler.missing_ranges()[0],
            (first.end_offset().unwrap(), third.offset),
        );
        assert_eq!(
            first.end_offset().unwrap(),
            second.offset,
        );
    }

    #[test]
    fn conflicting_overlap_is_rejected() {
        let input = b"abcdefghij".repeat(100);
        let descriptor = TransferDescriptor::from_bytes(&input);
        let key = key();

        let first = FragmentEnvelope {
            transfer_id: descriptor.transfer_id,
            offset: 0,
            total_len: descriptor.total_len,
            whole_digest: descriptor.whole_digest,
            payload: input[..100].to_vec(),
        };
        let overlap = FragmentEnvelope {
            transfer_id: descriptor.transfer_id,
            offset: 50,
            total_len: descriptor.total_len,
            whole_digest: descriptor.whole_digest,
            payload: input[50..150].to_vec(),
        };

        let mut assembler = FragmentAssembler::new(2_000);
        assembler.accept_wire(&first.seal(&key).unwrap(), &key).unwrap();
        assert_eq!(
            assembler
                .accept_wire(&overlap.seal(&key).unwrap(), &key)
                .unwrap_err(),
            FragmentError::OverlappingRange,
        );
    }

    #[test]
    fn wire_budget_enforces_real_envelope_overhead() {
        let key = key();
        assert_eq!(
            fragment_for_wire_budget(b"x", MIN_WIRE_BYTES, &key).unwrap_err(),
            FragmentError::WireBudgetTooSmall {
                minimum: MIN_WIRE_BYTES + 1,
                got: MIN_WIRE_BYTES,
            },
        );

        let wires =
            fragment_for_wire_budget(b"hello world", MIN_WIRE_BYTES + 3, &key)
                .unwrap();
        assert!(wires.iter().all(|wire| wire.len() <= MIN_WIRE_BYTES + 3));
    }
}
