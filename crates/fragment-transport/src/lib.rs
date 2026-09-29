use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

type HmacSha256 = Hmac<Sha256>;

pub const MAGIC: [u8; 4] = *b"SP3F";
pub const PARITY_MAGIC: [u8; 4] = *b"SP3E";
pub const RATELESS_MAGIC: [u8; 4] = *b"SP3R";
pub const VERSION: u8 = 1;
pub const TAG_BYTES: usize = 16;
pub const HEADER_BYTES: usize = 4 + 1 + 16 + 8 + 8 + 32 + 4;
pub const MIN_WIRE_BYTES: usize = HEADER_BYTES + TAG_BYTES;
pub const PARITY_HEADER_BYTES: usize = 4 + 1 + 16 + 8 + 32 + 4 + 8 + 4 + 2;
pub const MIN_PARITY_WIRE_BYTES: usize = PARITY_HEADER_BYTES + TAG_BYTES;
pub const RATELESS_HEADER_BYTES: usize = 4 + 1 + 16 + 8 + 32 + 2 + 4 + 8 + 4;
pub const MIN_RATELESS_WIRE_BYTES: usize = RATELESS_HEADER_BYTES + TAG_BYTES;
pub const MAX_RATELESS_SOURCE_SHARDS: usize = 256;

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


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityEnvelope {
    pub transfer_id: TransferId,
    pub total_len: u64,
    pub whole_digest: Digest32,
    pub stripe_index: u32,
    pub stripe_start_offset: u64,
    pub shard_payload_bytes: u32,
    pub data_count: u16,
    pub parity: Vec<u8>,
}

impl ParityEnvelope {
    pub fn seal(&self, key: &FragmentKey) -> Result<Vec<u8>, FragmentError> {
        validate_parity(self)?;

        let mut wire =
            Vec::with_capacity(PARITY_HEADER_BYTES + self.parity.len() + TAG_BYTES);
        wire.extend_from_slice(&PARITY_MAGIC);
        wire.push(VERSION);
        wire.extend_from_slice(&self.transfer_id);
        wire.extend_from_slice(&self.total_len.to_be_bytes());
        wire.extend_from_slice(&self.whole_digest);
        wire.extend_from_slice(&self.stripe_index.to_be_bytes());
        wire.extend_from_slice(&self.stripe_start_offset.to_be_bytes());
        wire.extend_from_slice(&self.shard_payload_bytes.to_be_bytes());
        wire.extend_from_slice(&self.data_count.to_be_bytes());
        wire.extend_from_slice(&self.parity);

        let tag = authentication_tag(&wire, key);
        wire.extend_from_slice(&tag);
        Ok(wire)
    }

    pub fn open(wire: &[u8], key: &FragmentKey) -> Result<Self, FragmentError> {
        if wire.len() < MIN_PARITY_WIRE_BYTES {
            return Err(FragmentError::Truncated);
        }

        let authenticated_len = wire.len() - TAG_BYTES;
        let provided_tag = &wire[authenticated_len..];
        let expected_tag = authentication_tag(&wire[..authenticated_len], key);
        if !constant_time_eq(provided_tag, &expected_tag) {
            return Err(FragmentError::AuthenticationFailed);
        }

        let mut cursor = Cursor::new(&wire[..authenticated_len]);
        if cursor.take(4)? != PARITY_MAGIC {
            return Err(FragmentError::WrongMagic);
        }
        let version = cursor.u8()?;
        if version != VERSION {
            return Err(FragmentError::WrongVersion(version));
        }

        let transfer_id = cursor.array16()?;
        let total_len = cursor.u64()?;
        let whole_digest = cursor.array32()?;
        let stripe_index = cursor.u32()?;
        let stripe_start_offset = cursor.u64()?;
        let shard_payload_bytes = cursor.u32()?;
        let data_count = cursor.u16()?;
        let parity = cursor
            .take(shard_payload_bytes as usize)?
            .to_vec();

        if cursor.remaining() != 0 {
            return Err(FragmentError::TrailingBytes);
        }

        let envelope = Self {
            transfer_id,
            total_len,
            whole_digest,
            stripe_index,
            stripe_start_offset,
            shard_payload_bytes,
            data_count,
            parity,
        };
        validate_parity(&envelope)?;
        Ok(envelope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErasureTransfer {
    pub data_wires: Vec<Vec<u8>>,
    pub parity_wires: Vec<Vec<u8>>,
    pub shard_payload_bytes: usize,
    pub stripe_width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParityRecoveryOutcome {
    NotNeeded,
    Recovered { offset: u64, payload_bytes: usize },
    Insufficient { missing_shards: usize },
}

/// Systematic one-parity-per-stripe erasure baseline.
///
/// All data shards remain normal authenticated SP3F fragments. Each SP3E
/// parity shard is the XOR of one stripe of padded data payloads. A stripe can
/// recover exactly one missing data shard; two or more missing shards remain
/// explicitly unrecoverable.
pub fn fragment_with_xor_parity(
    bytes: &[u8],
    wire_budget: usize,
    stripe_width: usize,
    key: &FragmentKey,
) -> Result<ErasureTransfer, FragmentError> {
    if stripe_width < 2 || stripe_width > u16::MAX as usize {
        return Err(FragmentError::InvalidStripeWidth);
    }

    let fixed_overhead = MIN_WIRE_BYTES.max(MIN_PARITY_WIRE_BYTES);
    if wire_budget <= fixed_overhead {
        return Err(FragmentError::WireBudgetTooSmall {
            minimum: fixed_overhead + 1,
            got: wire_budget,
        });
    }

    let shard_payload_bytes = wire_budget - fixed_overhead;
    let descriptor = TransferDescriptor::from_bytes(bytes);
    let mut data_wires = Vec::new();
    let mut parity_wires = Vec::new();

    if bytes.is_empty() {
        let envelope = FragmentEnvelope {
            transfer_id: descriptor.transfer_id,
            offset: 0,
            total_len: 0,
            whole_digest: descriptor.whole_digest,
            payload: Vec::new(),
        };
        data_wires.push(envelope.seal(key)?);
        return Ok(ErasureTransfer {
            data_wires,
            parity_wires,
            shard_payload_bytes,
            stripe_width,
        });
    }

    let chunks = bytes.chunks(shard_payload_bytes).collect::<Vec<_>>();

    for (index, payload) in chunks.iter().enumerate() {
        let offset = (index as u64)
            .checked_mul(shard_payload_bytes as u64)
            .ok_or(FragmentError::RangeOverflow)?;
        let envelope = FragmentEnvelope {
            transfer_id: descriptor.transfer_id,
            offset,
            total_len: descriptor.total_len,
            whole_digest: descriptor.whole_digest,
            payload: (*payload).to_vec(),
        };
        data_wires.push(envelope.seal(key)?);
    }

    for (stripe_index, stripe) in chunks.chunks(stripe_width).enumerate() {
        let mut parity = vec![0_u8; shard_payload_bytes];
        for shard in stripe {
            for (slot, &byte) in parity.iter_mut().zip(shard.iter()) {
                *slot ^= byte;
            }
        }

        let stripe_start_shard = stripe_index
            .checked_mul(stripe_width)
            .ok_or(FragmentError::RangeOverflow)?;
        let stripe_start_offset = (stripe_start_shard as u64)
            .checked_mul(shard_payload_bytes as u64)
            .ok_or(FragmentError::RangeOverflow)?;

        let envelope = ParityEnvelope {
            transfer_id: descriptor.transfer_id,
            total_len: descriptor.total_len,
            whole_digest: descriptor.whole_digest,
            stripe_index: u32::try_from(stripe_index)
                .map_err(|_| FragmentError::RangeOverflow)?,
            stripe_start_offset,
            shard_payload_bytes: u32::try_from(shard_payload_bytes)
                .map_err(|_| FragmentError::PayloadTooLarge)?,
            data_count: u16::try_from(stripe.len())
                .map_err(|_| FragmentError::PayloadTooLarge)?,
            parity,
        };
        let wire = envelope.seal(key)?;
        if wire.len() > wire_budget {
            return Err(FragmentError::WireBudgetTooSmall {
                minimum: wire.len(),
                got: wire_budget,
            });
        }
        parity_wires.push(wire);
    }

    Ok(ErasureTransfer {
        data_wires,
        parity_wires,
        shard_payload_bytes,
        stripe_width,
    })
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatelessSymbolEnvelope {
    pub transfer_id: TransferId,
    pub total_len: u64,
    pub whole_digest: Digest32,
    pub source_count: u16,
    pub shard_payload_bytes: u32,
    pub symbol_id: u64,
    pub payload: Vec<u8>,
}

impl RatelessSymbolEnvelope {
    pub fn seal(&self, key: &FragmentKey) -> Result<Vec<u8>, FragmentError> {
        validate_rateless_symbol(self)?;
        let payload_len = u32::try_from(self.payload.len())
            .map_err(|_| FragmentError::PayloadTooLarge)?;

        let mut wire = Vec::with_capacity(
            RATELESS_HEADER_BYTES + self.payload.len() + TAG_BYTES,
        );
        wire.extend_from_slice(&RATELESS_MAGIC);
        wire.push(VERSION);
        wire.extend_from_slice(&self.transfer_id);
        wire.extend_from_slice(&self.total_len.to_be_bytes());
        wire.extend_from_slice(&self.whole_digest);
        wire.extend_from_slice(&self.source_count.to_be_bytes());
        wire.extend_from_slice(&self.shard_payload_bytes.to_be_bytes());
        wire.extend_from_slice(&self.symbol_id.to_be_bytes());
        wire.extend_from_slice(&payload_len.to_be_bytes());
        wire.extend_from_slice(&self.payload);

        let tag = authentication_tag(&wire, key);
        wire.extend_from_slice(&tag);
        Ok(wire)
    }

    pub fn open(
        wire: &[u8],
        key: &FragmentKey,
    ) -> Result<Self, FragmentError> {
        if wire.len() < MIN_RATELESS_WIRE_BYTES {
            return Err(FragmentError::Truncated);
        }

        let authenticated_len = wire.len() - TAG_BYTES;
        let provided_tag = &wire[authenticated_len..];
        let expected_tag = authentication_tag(&wire[..authenticated_len], key);
        if !constant_time_eq(provided_tag, &expected_tag) {
            return Err(FragmentError::AuthenticationFailed);
        }

        let mut cursor = Cursor::new(&wire[..authenticated_len]);
        if cursor.take(4)? != RATELESS_MAGIC {
            return Err(FragmentError::WrongMagic);
        }
        let version = cursor.u8()?;
        if version != VERSION {
            return Err(FragmentError::WrongVersion(version));
        }

        let envelope = Self {
            transfer_id: cursor.array16()?,
            total_len: cursor.u64()?,
            whole_digest: cursor.array32()?,
            source_count: cursor.u16()?,
            shard_payload_bytes: cursor.u32()?,
            symbol_id: cursor.u64()?,
            payload: {
                let payload_len = cursor.u32()? as usize;
                cursor.take(payload_len)?.to_vec()
            },
        };

        if cursor.remaining() != 0 {
            return Err(FragmentError::TrailingBytes);
        }
        validate_rateless_symbol(&envelope)?;
        Ok(envelope)
    }
}

/// Generates one authenticated SP3R symbol.
///
/// The stream is systematic for symbol IDs 0..K. IDs >= K generate
/// deterministic random-linear XOR repair equations. There is no fixed repair
/// count: the sender can keep increasing symbol_id until the receiver reports
/// full rank.
pub fn rateless_symbol_for_wire_budget(
    bytes: &[u8],
    wire_budget: usize,
    symbol_id: u64,
    key: &FragmentKey,
) -> Result<Vec<u8>, FragmentError> {
    if bytes.is_empty() {
        return Err(FragmentError::RatelessEmptyTransfer);
    }
    if wire_budget <= MIN_RATELESS_WIRE_BYTES {
        return Err(FragmentError::WireBudgetTooSmall {
            minimum: MIN_RATELESS_WIRE_BYTES + 1,
            got: wire_budget,
        });
    }

    let shard_payload_bytes = wire_budget - MIN_RATELESS_WIRE_BYTES;
    let source_count = bytes.len().div_ceil(shard_payload_bytes);
    if source_count == 0 || source_count > MAX_RATELESS_SOURCE_SHARDS {
        return Err(FragmentError::ResourceLimit);
    }

    let descriptor = TransferDescriptor::from_bytes(bytes);
    let coefficients = rateless_coefficients(
        descriptor.transfer_id,
        symbol_id,
        source_count,
    );
    let mut payload = vec![0_u8; shard_payload_bytes];

    for source_index in 0..source_count {
        if !coefficient_is_set(&coefficients, source_index) {
            continue;
        }
        let start = source_index
            .checked_mul(shard_payload_bytes)
            .ok_or(FragmentError::RangeOverflow)?;
        let end = start
            .saturating_add(shard_payload_bytes)
            .min(bytes.len());
        for (target, &value) in payload
            .iter_mut()
            .zip(bytes[start..end].iter())
        {
            *target ^= value;
        }
    }

    RatelessSymbolEnvelope {
        transfer_id: descriptor.transfer_id,
        total_len: descriptor.total_len,
        whole_digest: descriptor.whole_digest,
        source_count: u16::try_from(source_count)
            .map_err(|_| FragmentError::ResourceLimit)?,
        shard_payload_bytes: u32::try_from(shard_payload_bytes)
            .map_err(|_| FragmentError::PayloadTooLarge)?,
        symbol_id,
        payload,
    }
    .seal(key)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RatelessDescriptor {
    transfer_id: TransferId,
    total_len: u64,
    whole_digest: Digest32,
    source_count: usize,
    shard_payload_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RatelessRow {
    coefficients: Vec<u64>,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatelessAcceptOutcome {
    Innovative { rank: usize },
    Dependent { rank: usize },
    DuplicateSymbol { rank: usize },
}

pub struct RatelessDecoder {
    max_total_len: u64,
    descriptor: Option<RatelessDescriptor>,
    rows: BTreeMap<usize, RatelessRow>,
    seen_symbols: BTreeMap<u64, Digest32>,
}

impl RatelessDecoder {
    pub fn new(max_total_len: u64) -> Self {
        Self {
            max_total_len,
            descriptor: None,
            rows: BTreeMap::new(),
            seen_symbols: BTreeMap::new(),
        }
    }

    pub fn rank(&self) -> usize {
        self.rows.len()
    }

    pub fn source_count(&self) -> Option<usize> {
        self.descriptor.as_ref().map(|value| value.source_count)
    }

    pub fn is_decodable(&self) -> bool {
        self.source_count()
            .is_some_and(|source_count| self.rank() == source_count)
    }

    pub fn accept_wire(
        &mut self,
        wire: &[u8],
        key: &FragmentKey,
    ) -> Result<RatelessAcceptOutcome, FragmentError> {
        let envelope = RatelessSymbolEnvelope::open(wire, key)?;
        let wire_digest = sha256(wire);

        if let Some(existing) = self.seen_symbols.get(&envelope.symbol_id) {
            return if existing == &wire_digest {
                Ok(RatelessAcceptOutcome::DuplicateSymbol {
                    rank: self.rank(),
                })
            } else {
                Err(FragmentError::ConflictingRatelessSymbol)
            };
        }

        if envelope.total_len > self.max_total_len {
            return Err(FragmentError::ResourceLimit);
        }

        let descriptor = RatelessDescriptor {
            transfer_id: envelope.transfer_id,
            total_len: envelope.total_len,
            whole_digest: envelope.whole_digest,
            source_count: envelope.source_count as usize,
            shard_payload_bytes: envelope.shard_payload_bytes as usize,
        };

        match &self.descriptor {
            None => self.descriptor = Some(descriptor.clone()),
            Some(existing) if existing == &descriptor => {}
            Some(_) => return Err(FragmentError::TransferMismatch),
        }

        self.seen_symbols
            .insert(envelope.symbol_id, wire_digest);

        let mut row = RatelessRow {
            coefficients: rateless_coefficients(
                envelope.transfer_id,
                envelope.symbol_id,
                envelope.source_count as usize,
            ),
            payload: envelope.payload,
        };

        for (&pivot, basis) in &self.rows {
            if coefficient_is_set(&row.coefficients, pivot) {
                xor_coefficients(
                    &mut row.coefficients,
                    &basis.coefficients,
                );
                xor_bytes(&mut row.payload, &basis.payload);
            }
        }

        let Some(pivot) = first_set_coefficient(
            &row.coefficients,
            descriptor.source_count,
        ) else {
            if row.payload.iter().any(|&byte| byte != 0) {
                return Err(FragmentError::InconsistentRatelessEquation);
            }
            return Ok(RatelessAcceptOutcome::Dependent {
                rank: self.rank(),
            });
        };

        self.rows.insert(pivot, row);
        Ok(RatelessAcceptOutcome::Innovative {
            rank: self.rank(),
        })
    }

    pub fn reconstruct(&self) -> Result<Vec<u8>, FragmentError> {
        let descriptor = self
            .descriptor
            .as_ref()
            .ok_or(FragmentError::Incomplete)?;
        if self.rank() != descriptor.source_count {
            return Err(FragmentError::Incomplete);
        }

        let mut shards =
            vec![None::<Vec<u8>>; descriptor.source_count];

        for pivot in (0..descriptor.source_count).rev() {
            let row = self
                .rows
                .get(&pivot)
                .ok_or(FragmentError::Incomplete)?;
            let mut recovered = row.payload.clone();

            for (source_index, known) in shards
                .iter()
                .enumerate()
                .skip(pivot + 1)
            {
                if coefficient_is_set(
                    &row.coefficients,
                    source_index,
                ) {
                    let known = known
                        .as_ref()
                        .ok_or(FragmentError::Incomplete)?;
                    xor_bytes(&mut recovered, known);
                }
            }

            shards[pivot] = Some(recovered);
        }

        let capacity = usize::try_from(descriptor.total_len)
            .map_err(|_| FragmentError::ResourceLimit)?;
        let mut output = Vec::with_capacity(capacity);
        for shard in shards {
            output.extend_from_slice(
                &shard.ok_or(FragmentError::Incomplete)?,
            );
        }
        output.truncate(capacity);

        if output.len() != capacity {
            return Err(FragmentError::Incomplete);
        }
        if sha256(&output) != descriptor.whole_digest {
            return Err(FragmentError::WholeDigestMismatch);
        }

        Ok(output)
    }
}

fn rateless_coefficients(
    transfer_id: TransferId,
    symbol_id: u64,
    source_count: usize,
) -> Vec<u64> {
    let mut coefficients =
        vec![0_u64; source_count.div_ceil(64)];

    if (symbol_id as u128) < source_count as u128 {
        set_coefficient(
            &mut coefficients,
            symbol_id as usize,
        );
        return coefficients;
    }

    let mut hasher = Sha256::new();
    hasher.update(b"SP3R-COEFFICIENT-V1");
    hasher.update(transfer_id);
    hasher.update(symbol_id.to_be_bytes());
    let digest = hasher.finalize();
    let mut seed_bytes = [0_u8; 8];
    seed_bytes.copy_from_slice(&digest[..8]);
    let mut state = u64::from_be_bytes(seed_bytes);
    if state == 0 {
        state = 0x9e37_79b9_7f4a_7c15;
    }

    let mut any = false;
    for source_index in 0..source_count {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        if state & 1 == 1 {
            set_coefficient(&mut coefficients, source_index);
            any = true;
        }
    }

    if !any {
        set_coefficient(
            &mut coefficients,
            symbol_id as usize % source_count,
        );
    }

    coefficients
}

fn set_coefficient(coefficients: &mut [u64], index: usize) {
    coefficients[index / 64] |= 1_u64 << (index % 64);
}

fn coefficient_is_set(coefficients: &[u64], index: usize) -> bool {
    coefficients
        .get(index / 64)
        .is_some_and(|word| word & (1_u64 << (index % 64)) != 0)
}

fn first_set_coefficient(
    coefficients: &[u64],
    source_count: usize,
) -> Option<usize> {
    for (word_index, &word) in coefficients.iter().enumerate() {
        if word == 0 {
            continue;
        }
        let bit = word.trailing_zeros() as usize;
        let index = word_index * 64 + bit;
        if index < source_count {
            return Some(index);
        }
    }
    None
}

fn xor_coefficients(target: &mut [u64], source: &[u64]) {
    for (left, right) in target.iter_mut().zip(source) {
        *left ^= *right;
    }
}

fn xor_bytes(target: &mut [u8], source: &[u8]) {
    for (left, right) in target.iter_mut().zip(source) {
        *left ^= *right;
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FragmentProvenance {
    pub source_id: String,
    pub carrier: String,
    pub observed_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceSummary {
    pub unique_sources: Vec<String>,
    pub unique_carriers: Vec<String>,
    pub first_observed_at_ms: u64,
    pub last_observed_at_ms: u64,
    pub fragments_tracked: usize,
    pub fragment_offsets_with_multiple_sources: usize,
}

pub struct ProvenanceAssembler {
    inner: FragmentAssembler,
    provenance_by_offset: BTreeMap<u64, BTreeSet<FragmentProvenance>>,
}

impl ProvenanceAssembler {
    pub fn new(max_total_len: u64) -> Self {
        Self {
            inner: FragmentAssembler::new(max_total_len),
            provenance_by_offset: BTreeMap::new(),
        }
    }

    pub fn accept_wire_from(
        &mut self,
        wire: &[u8],
        key: &FragmentKey,
        provenance: FragmentProvenance,
    ) -> Result<AcceptOutcome, FragmentError> {
        validate_provenance(&provenance)?;
        let envelope = FragmentEnvelope::open(wire, key)?;
        let offset = envelope.offset;
        let outcome = self.inner.accept(envelope)?;

        self.provenance_by_offset
            .entry(offset)
            .or_default()
            .insert(provenance);

        Ok(outcome)
    }

    pub fn is_complete(&self) -> bool {
        self.inner.is_complete()
    }

    pub fn reconstruct_with_summary(
        &self,
    ) -> Result<(Vec<u8>, ProvenanceSummary), FragmentError> {
        let bytes = self.inner.reconstruct()?;
        if self.provenance_by_offset.is_empty() {
            return Err(FragmentError::InvalidProvenance);
        }

        let mut sources = BTreeSet::new();
        let mut carriers = BTreeSet::new();
        let mut first_observed = u64::MAX;
        let mut last_observed = 0_u64;
        let mut multi_source_offsets = 0_usize;

        for provenances in self.provenance_by_offset.values() {
            let mut offset_sources = BTreeSet::new();

            for provenance in provenances {
                sources.insert(provenance.source_id.clone());
                carriers.insert(provenance.carrier.clone());
                offset_sources.insert(provenance.source_id.clone());
                first_observed =
                    first_observed.min(provenance.observed_at_ms);
                last_observed =
                    last_observed.max(provenance.observed_at_ms);
            }

            if offset_sources.len() > 1 {
                multi_source_offsets += 1;
            }
        }

        Ok((
            bytes,
            ProvenanceSummary {
                unique_sources: sources.into_iter().collect(),
                unique_carriers: carriers.into_iter().collect(),
                first_observed_at_ms: first_observed,
                last_observed_at_ms: last_observed,
                fragments_tracked: self.provenance_by_offset.len(),
                fragment_offsets_with_multiple_sources: multi_source_offsets,
            },
        ))
    }
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

    pub fn recover_with_parity_wire(
        &mut self,
        wire: &[u8],
        key: &FragmentKey,
    ) -> Result<ParityRecoveryOutcome, FragmentError> {
        let parity = ParityEnvelope::open(wire, key)?;
        self.recover_with_parity(parity)
    }

    pub fn recover_with_parity(
        &mut self,
        parity: ParityEnvelope,
    ) -> Result<ParityRecoveryOutcome, FragmentError> {
        validate_parity(&parity)?;
        if parity.total_len > self.max_total_len {
            return Err(FragmentError::ResourceLimit);
        }

        let descriptor = TransferDescriptor {
            transfer_id: parity.transfer_id,
            total_len: parity.total_len,
            whole_digest: parity.whole_digest,
        };

        match &self.descriptor {
            None => self.descriptor = Some(descriptor),
            Some(existing) if existing == &descriptor => {}
            Some(_) => return Err(FragmentError::TransferMismatch),
        }

        if parity.total_len == 0 {
            return Ok(ParityRecoveryOutcome::NotNeeded);
        }

        let shard_bytes = parity.shard_payload_bytes as u64;
        let mut missing = Vec::new();

        for index in 0..parity.data_count as u64 {
            let offset = parity
                .stripe_start_offset
                .checked_add(
                    index
                        .checked_mul(shard_bytes)
                        .ok_or(FragmentError::RangeOverflow)?,
                )
                .ok_or(FragmentError::RangeOverflow)?;

            if offset >= parity.total_len {
                return Err(FragmentError::InvalidParity);
            }

            let expected_len = usize::try_from(
                shard_bytes.min(parity.total_len - offset),
            )
            .map_err(|_| FragmentError::ResourceLimit)?;

            match self.fragments.get(&offset) {
                Some(payload) if payload.len() == expected_len => {}
                Some(_) => return Err(FragmentError::InvalidParity),
                None => missing.push((offset, expected_len)),
            }
        }

        if missing.is_empty() {
            return Ok(ParityRecoveryOutcome::NotNeeded);
        }
        if missing.len() > 1 {
            return Ok(ParityRecoveryOutcome::Insufficient {
                missing_shards: missing.len(),
            });
        }

        let (missing_offset, missing_len) = missing[0];
        let mut recovered = parity.parity.clone();

        for index in 0..parity.data_count as u64 {
            let offset = parity
                .stripe_start_offset
                .checked_add(
                    index
                        .checked_mul(shard_bytes)
                        .ok_or(FragmentError::RangeOverflow)?,
                )
                .ok_or(FragmentError::RangeOverflow)?;
            if offset == missing_offset {
                continue;
            }
            let payload = self
                .fragments
                .get(&offset)
                .ok_or(FragmentError::InvalidParity)?;
            for (slot, &byte) in recovered.iter_mut().zip(payload.iter()) {
                *slot ^= byte;
            }
        }

        recovered.truncate(missing_len);
        let outcome = self.accept(FragmentEnvelope {
            transfer_id: parity.transfer_id,
            offset: missing_offset,
            total_len: parity.total_len,
            whole_digest: parity.whole_digest,
            payload: recovered,
        })?;
        if outcome != AcceptOutcome::Accepted {
            return Err(FragmentError::InvalidParity);
        }

        Ok(ParityRecoveryOutcome::Recovered {
            offset: missing_offset,
            payload_bytes: missing_len,
        })
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
    InvalidStripeWidth,
    InvalidParity,
    InvalidProvenance,
    RatelessEmptyTransfer,
    InvalidRatelessSymbol,
    ConflictingRatelessSymbol,
    InconsistentRatelessEquation,
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
            Self::InvalidStripeWidth => write!(f, "erasure stripe width is invalid"),
            Self::InvalidParity => write!(f, "erasure parity metadata is invalid"),
            Self::InvalidProvenance => {
                write!(f, "fragment provenance metadata is invalid")
            }
            Self::RatelessEmptyTransfer => {
                write!(f, "rateless transfer cannot encode an empty object")
            }
            Self::InvalidRatelessSymbol => {
                write!(f, "rateless symbol metadata is invalid")
            }
            Self::ConflictingRatelessSymbol => {
                write!(f, "same rateless symbol ID carried different authenticated bytes")
            }
            Self::InconsistentRatelessEquation => {
                write!(f, "rateless equation reduced to zero coefficients with nonzero payload")
            }
        }
    }
}

impl std::error::Error for FragmentError {}

pub fn sha256(bytes: &[u8]) -> Digest32 {
    Sha256::digest(bytes).into()
}

fn validate_rateless_symbol(
    envelope: &RatelessSymbolEnvelope,
) -> Result<(), FragmentError> {
    let source_count = envelope.source_count as usize;
    let shard_payload_bytes = envelope.shard_payload_bytes as usize;

    if envelope.total_len == 0
        || source_count == 0
        || source_count > MAX_RATELESS_SOURCE_SHARDS
        || shard_payload_bytes == 0
        || envelope.payload.len() != shard_payload_bytes
    {
        return Err(FragmentError::InvalidRatelessSymbol);
    }

    let total_len = usize::try_from(envelope.total_len)
        .map_err(|_| FragmentError::ResourceLimit)?;
    if total_len.div_ceil(shard_payload_bytes) != source_count {
        return Err(FragmentError::InvalidRatelessSymbol);
    }

    Ok(())
}

fn validate_provenance(
    provenance: &FragmentProvenance,
) -> Result<(), FragmentError> {
    if provenance.source_id.trim().is_empty()
        || provenance.carrier.trim().is_empty()
    {
        return Err(FragmentError::InvalidProvenance);
    }
    Ok(())
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

fn validate_parity(envelope: &ParityEnvelope) -> Result<(), FragmentError> {
    if envelope.shard_payload_bytes == 0
        || envelope.data_count < 1
        || envelope.parity.len() != envelope.shard_payload_bytes as usize
    {
        return Err(FragmentError::InvalidParity);
    }

    if envelope.total_len == 0 {
        return Err(FragmentError::InvalidParity);
    }

    if envelope.stripe_start_offset >= envelope.total_len {
        return Err(FragmentError::InvalidParity);
    }

    let stripe_capacity = (envelope.data_count as u64)
        .checked_mul(envelope.shard_payload_bytes as u64)
        .ok_or(FragmentError::RangeOverflow)?;
    let stripe_end = envelope
        .stripe_start_offset
        .checked_add(stripe_capacity)
        .ok_or(FragmentError::RangeOverflow)?;

    if stripe_end.saturating_sub(envelope.shard_payload_bytes as u64)
        >= envelope.total_len
    {
        return Err(FragmentError::InvalidParity);
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

    fn u16(&mut self) -> Result<u16, FragmentError> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| FragmentError::Truncated)?,
        ))
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
    fn xor_parity_recovers_one_missing_shard_per_stripe() {
        let input = (0..96 * 1024)
            .map(|index| ((index * 17 + 31) % 251) as u8)
            .collect::<Vec<_>>();
        let key = key();
        let transfer =
            fragment_with_xor_parity(&input, 240, 4, &key).unwrap();

        assert!(!transfer.parity_wires.is_empty());
        assert!(transfer
            .data_wires
            .iter()
            .chain(transfer.parity_wires.iter())
            .all(|wire| wire.len() <= 240));

        let mut assembler = FragmentAssembler::new(128 * 1024);

        for (index, wire) in transfer.data_wires.iter().enumerate() {
            if index % transfer.stripe_width == 1 {
                continue;
            }
            assembler.accept_wire(wire, &key).unwrap();
        }

        let mut recovered = 0_usize;
        for parity in &transfer.parity_wires {
            if matches!(
                assembler.recover_with_parity_wire(parity, &key).unwrap(),
                ParityRecoveryOutcome::Recovered { .. }
            ) {
                recovered += 1;
            }
        }

        assert_eq!(recovered, transfer.parity_wires.len());
        assert!(assembler.is_complete());
        assert_eq!(assembler.reconstruct().unwrap(), input);
    }

    #[test]
    fn xor_parity_refuses_two_missing_shards_in_same_stripe() {
        let input = (0..16 * 1024)
            .map(|index| (index % 239) as u8)
            .collect::<Vec<_>>();
        let key = key();
        let transfer =
            fragment_with_xor_parity(&input, 220, 4, &key).unwrap();

        let mut assembler = FragmentAssembler::new(32 * 1024);
        for (index, wire) in transfer.data_wires.iter().enumerate() {
            if index == 0 || index == 1 {
                continue;
            }
            assembler.accept_wire(wire, &key).unwrap();
        }

        assert_eq!(
            assembler
                .recover_with_parity_wire(&transfer.parity_wires[0], &key)
                .unwrap(),
            ParityRecoveryOutcome::Insufficient { missing_shards: 2 },
        );
        assert!(!assembler.is_complete());
    }

    #[test]
    fn tampered_parity_is_rejected_before_recovery() {
        let input = b"parity-authentication".repeat(1024);
        let key = key();
        let mut transfer =
            fragment_with_xor_parity(&input, 220, 3, &key).unwrap();

        let last = transfer.parity_wires[0].len() - 1;
        transfer.parity_wires[0][last] ^= 1;

        let mut assembler = FragmentAssembler::new(64 * 1024);
        assert_eq!(
            assembler
                .recover_with_parity_wire(&transfer.parity_wires[0], &key)
                .unwrap_err(),
            FragmentError::AuthenticationFailed,
        );
    }

    #[test]
    fn rateless_repairs_multiple_missing_systematic_shards() {
        let input = (0..12 * 1024)
            .map(|index| ((index * 29 + 7) % 251) as u8)
            .collect::<Vec<_>>();
        let key = key();

        let first =
            rateless_symbol_for_wire_budget(&input, 220, 0, &key).unwrap();
        let first_envelope =
            RatelessSymbolEnvelope::open(&first, &key).unwrap();
        let source_count = first_envelope.source_count as usize;

        let mut decoder = RatelessDecoder::new(32 * 1024);
        let mut delivered = 0_usize;

        // Deliberately lose every fourth systematic source shard.
        for symbol_id in 0..source_count as u64 {
            if symbol_id.is_multiple_of(4) {
                continue;
            }
            let wire = rateless_symbol_for_wire_budget(
                &input,
                220,
                symbol_id,
                &key,
            )
            .unwrap();
            decoder.accept_wire(&wire, &key).unwrap();
            delivered += 1;
        }
        assert!(!decoder.is_decodable());

        let mut symbol_id = source_count as u64;
        let max_symbol_id = source_count as u64 * 8;
        while !decoder.is_decodable() && symbol_id < max_symbol_id {
            // Model additional repair-symbol loss without changing the code.
            if !symbol_id.is_multiple_of(7) {
                let wire = rateless_symbol_for_wire_budget(
                    &input,
                    220,
                    symbol_id,
                    &key,
                )
                .unwrap();
                decoder.accept_wire(&wire, &key).unwrap();
                delivered += 1;
            }
            symbol_id += 1;
        }

        assert!(decoder.is_decodable());
        assert_eq!(decoder.rank(), source_count);
        assert_eq!(decoder.reconstruct().unwrap(), input);
        assert!(delivered >= source_count);
    }

    #[test]
    fn rateless_duplicate_symbol_is_idempotent() {
        let input = b"rateless duplicate".repeat(100);
        let key = key();
        let wire =
            rateless_symbol_for_wire_budget(&input, 220, 0, &key).unwrap();
        let mut decoder = RatelessDecoder::new(8 * 1024);

        assert!(matches!(
            decoder.accept_wire(&wire, &key).unwrap(),
            RatelessAcceptOutcome::Innovative { .. },
        ));
        let rank = decoder.rank();
        assert_eq!(
            decoder.accept_wire(&wire, &key).unwrap(),
            RatelessAcceptOutcome::DuplicateSymbol { rank },
        );
    }

    #[test]
    fn rateless_tampering_is_rejected() {
        let input = b"rateless authentication".repeat(100);
        let key = key();
        let mut wire =
            rateless_symbol_for_wire_budget(&input, 220, 0, &key).unwrap();
        let last = wire.len() - 1;
        wire[last] ^= 1;

        let mut decoder = RatelessDecoder::new(8 * 1024);
        assert_eq!(
            decoder.accept_wire(&wire, &key).unwrap_err(),
            FragmentError::AuthenticationFailed,
        );
    }

    #[test]
    fn rateless_transfer_descriptor_mismatch_is_rejected() {
        let key = key();
        let first =
            rateless_symbol_for_wire_budget(b"first object", 220, 0, &key)
                .unwrap();
        let second =
            rateless_symbol_for_wire_budget(b"second object", 220, 0, &key)
                .unwrap();

        let mut decoder = RatelessDecoder::new(1_024);
        decoder.accept_wire(&first, &key).unwrap();
        assert_eq!(
            decoder.accept_wire(&second, &key).unwrap_err(),
            FragmentError::TransferMismatch,
        );
    }

    #[test]
    fn provenance_assembler_merges_fragments_from_multiple_sources() {
        let input = (0..8 * 1024)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let key = key();
        let wires = fragment_for_wire_budget(&input, 220, &key).unwrap();

        let mut assembler = ProvenanceAssembler::new(16 * 1024);
        for (index, wire) in wires.iter().enumerate() {
            let (source_id, carrier) = match index % 3 {
                0 => ("peer-a", "ble-gatt"),
                1 => ("peer-b", "wifi-direct"),
                _ => ("peer-c", "acoustic"),
            };
            assembler
                .accept_wire_from(
                    wire,
                    &key,
                    FragmentProvenance {
                        source_id: source_id.to_owned(),
                        carrier: carrier.to_owned(),
                        observed_at_ms: 1_000 + index as u64,
                    },
                )
                .unwrap();
        }

        assert_eq!(
            assembler
                .accept_wire_from(
                    &wires[0],
                    &key,
                    FragmentProvenance {
                        source_id: "peer-d".to_owned(),
                        carrier: "nfc".to_owned(),
                        observed_at_ms: 9_999,
                    },
                )
                .unwrap(),
            AcceptOutcome::Duplicate,
        );

        let (output, summary) = assembler.reconstruct_with_summary().unwrap();
        assert_eq!(output, input);
        assert_eq!(
            summary.unique_sources,
            vec![
                "peer-a".to_owned(),
                "peer-b".to_owned(),
                "peer-c".to_owned(),
                "peer-d".to_owned(),
            ],
        );
        assert!(summary.unique_carriers.contains(&"acoustic".to_owned()));
        assert!(summary.unique_carriers.contains(&"nfc".to_owned()));
        assert_eq!(summary.fragment_offsets_with_multiple_sources, 1);
        assert_eq!(summary.fragments_tracked, wires.len());
    }

    #[test]
    fn provenance_does_not_merge_mismatched_transfers() {
        let key = key();
        let first =
            fragment_for_wire_budget(b"first transfer", 220, &key).unwrap();
        let second =
            fragment_for_wire_budget(b"second transfer", 220, &key).unwrap();

        let mut assembler = ProvenanceAssembler::new(1_024);
        assembler
            .accept_wire_from(
                &first[0],
                &key,
                FragmentProvenance {
                    source_id: "peer-a".to_owned(),
                    carrier: "ble".to_owned(),
                    observed_at_ms: 1,
                },
            )
            .unwrap();

        assert_eq!(
            assembler
                .accept_wire_from(
                    &second[0],
                    &key,
                    FragmentProvenance {
                        source_id: "peer-b".to_owned(),
                        carrier: "wifi".to_owned(),
                        observed_at_ms: 2,
                    },
                )
                .unwrap_err(),
            FragmentError::TransferMismatch,
        );
    }

    #[test]
    fn provenance_requires_nonempty_source_and_carrier() {
        let key = key();
        let wire =
            fragment_for_wire_budget(b"payload", 220, &key).unwrap();
        let mut assembler = ProvenanceAssembler::new(1_024);

        assert_eq!(
            assembler
                .accept_wire_from(
                    &wire[0],
                    &key,
                    FragmentProvenance {
                        source_id: String::new(),
                        carrier: "ble".to_owned(),
                        observed_at_ms: 1,
                    },
                )
                .unwrap_err(),
            FragmentError::InvalidProvenance,
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
