use sha2::{Digest, Sha256};
use std::fmt;

pub const SEGMENT_MAGIC: [u8; 4] = *b"S3MG";
pub const SEGMENT_VERSION: u8 = 1;

/// Conservative prototype budget for one binary/data-SMS payload.
///
/// This is a project transport budget, not a claim that every carrier/operator
/// exposes exactly this many application bytes.
pub const SMS_DATA_BUDGET_BYTES: usize = 120;
pub const SEGMENT_HEADER_BYTES: usize = 4 + 1 + 8 + 1 + 1 + 1;
pub const SEGMENT_PAYLOAD_BYTES: usize =
    SMS_DATA_BUDGET_BYTES - SEGMENT_HEADER_BYTES;
pub const DEFAULT_MAX_SEGMENTS: u8 = 16;
pub const DEFAULT_MAX_REASSEMBLED_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsSegment {
    pub transfer_id: u64,
    pub index: u8,
    pub count: u8,
    pub payload: Vec<u8>,
}

impl SmsSegment {
    pub fn encode(&self) -> Result<Vec<u8>, SmsError> {
        validate_segment(self)?;
        let payload_len = u8::try_from(self.payload.len())
            .map_err(|_| SmsError::PayloadTooLarge)?;

        let mut wire = Vec::with_capacity(
            SEGMENT_HEADER_BYTES + self.payload.len(),
        );
        wire.extend_from_slice(&SEGMENT_MAGIC);
        wire.push(SEGMENT_VERSION);
        wire.extend_from_slice(&self.transfer_id.to_be_bytes());
        wire.push(self.index);
        wire.push(self.count);
        wire.push(payload_len);
        wire.extend_from_slice(&self.payload);

        if wire.len() > SMS_DATA_BUDGET_BYTES {
            return Err(SmsError::PayloadTooLarge);
        }
        Ok(wire)
    }

    pub fn decode(wire: &[u8]) -> Result<Self, SmsError> {
        if wire.len() < SEGMENT_HEADER_BYTES {
            return Err(SmsError::Truncated);
        }
        if wire.len() > SMS_DATA_BUDGET_BYTES {
            return Err(SmsError::PayloadTooLarge);
        }

        let mut cursor = Cursor::new(wire);
        if cursor.take(4)? != SEGMENT_MAGIC {
            return Err(SmsError::WrongMagic);
        }
        let version = cursor.u8()?;
        if version != SEGMENT_VERSION {
            return Err(SmsError::WrongVersion(version));
        }

        let transfer_id = cursor.u64()?;
        let index = cursor.u8()?;
        let count = cursor.u8()?;
        let payload_len = cursor.u8()? as usize;
        let payload = cursor.take(payload_len)?.to_vec();

        if cursor.remaining() != 0 {
            return Err(SmsError::TrailingBytes);
        }

        let segment = Self {
            transfer_id,
            index,
            count,
            payload,
        };
        validate_segment(&segment)?;
        Ok(segment)
    }
}

pub fn transfer_id(bytes: &[u8]) -> u64 {
    let digest = Sha256::digest(bytes);
    u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 digest always has at least eight bytes"),
    )
}

pub fn segment_capsule(bytes: &[u8]) -> Result<Vec<SmsSegment>, SmsError> {
    if bytes.is_empty() {
        return Err(SmsError::EmptyTransfer);
    }
    if bytes.len() > DEFAULT_MAX_REASSEMBLED_BYTES {
        return Err(SmsError::TransferTooLarge);
    }

    let count = bytes.len().div_ceil(SEGMENT_PAYLOAD_BYTES);
    if count == 0 || count > DEFAULT_MAX_SEGMENTS as usize {
        return Err(SmsError::TooManySegments);
    }
    let count_u8 =
        u8::try_from(count).map_err(|_| SmsError::TooManySegments)?;
    let id = transfer_id(bytes);

    Ok(bytes
        .chunks(SEGMENT_PAYLOAD_BYTES)
        .enumerate()
        .map(|(index, payload)| SmsSegment {
            transfer_id: id,
            index: index as u8,
            count: count_u8,
            payload: payload.to_vec(),
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptOutcome {
    Incomplete,
    Duplicate,
    Complete(Vec<u8>),
}

#[derive(Debug)]
pub struct SmsAssembler {
    max_segments: u8,
    max_reassembled_bytes: usize,
    transfer_id: Option<u64>,
    count: Option<u8>,
    parts: Vec<Option<Vec<u8>>>,
    received_bytes: usize,
}

impl SmsAssembler {
    pub fn new(max_segments: u8, max_reassembled_bytes: usize) -> Self {
        Self {
            max_segments,
            max_reassembled_bytes,
            transfer_id: None,
            count: None,
            parts: Vec::new(),
            received_bytes: 0,
        }
    }

    pub fn conservative() -> Self {
        Self::new(
            DEFAULT_MAX_SEGMENTS,
            DEFAULT_MAX_REASSEMBLED_BYTES,
        )
    }

    pub fn accept(
        &mut self,
        segment: SmsSegment,
    ) -> Result<AcceptOutcome, SmsError> {
        validate_segment(&segment)?;

        if segment.count > self.max_segments {
            return Err(SmsError::TooManySegments);
        }

        match (self.transfer_id, self.count) {
            (None, None) => {
                self.transfer_id = Some(segment.transfer_id);
                self.count = Some(segment.count);
                self.parts = vec![None; segment.count as usize];
            }
            (Some(id), Some(count))
                if id == segment.transfer_id && count == segment.count => {}
            _ => return Err(SmsError::TransferMismatch),
        }

        let slot = &mut self.parts[segment.index as usize];
        if let Some(existing) = slot {
            if existing == &segment.payload {
                return Ok(AcceptOutcome::Duplicate);
            }
            return Err(SmsError::ConflictingDuplicate);
        }

        let new_total = self
            .received_bytes
            .checked_add(segment.payload.len())
            .ok_or(SmsError::TransferTooLarge)?;
        if new_total > self.max_reassembled_bytes {
            return Err(SmsError::TransferTooLarge);
        }

        self.received_bytes = new_total;
        *slot = Some(segment.payload);

        if self.parts.iter().any(Option::is_none) {
            return Ok(AcceptOutcome::Incomplete);
        }

        let mut bytes = Vec::with_capacity(self.received_bytes);
        for part in &self.parts {
            bytes.extend_from_slice(
                part.as_deref()
                    .expect("all parts checked present above"),
            );
        }

        if Some(transfer_id(&bytes)) != self.transfer_id {
            return Err(SmsError::TransferDigestMismatch);
        }

        Ok(AcceptOutcome::Complete(bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmsSendPolicy {
    pub user_consented: bool,
    pub subscription_available: bool,
    pub roaming: bool,
    pub allow_roaming: bool,
    pub max_segments: u8,
}

impl SmsSendPolicy {
    pub fn authorize(&self, segment_count: usize) -> Result<(), SmsError> {
        if !self.user_consented {
            return Err(SmsError::UserConsentRequired);
        }
        if !self.subscription_available {
            return Err(SmsError::SubscriptionUnavailable);
        }
        if self.roaming && !self.allow_roaming {
            return Err(SmsError::RoamingBlocked);
        }
        if segment_count == 0 || segment_count > self.max_segments as usize {
            return Err(SmsError::PolicySegmentLimit);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmsError {
    Truncated,
    WrongMagic,
    WrongVersion(u8),
    EmptyTransfer,
    EmptySegment,
    PayloadTooLarge,
    TooManySegments,
    InvalidSegmentIndex,
    TrailingBytes,
    TransferMismatch,
    ConflictingDuplicate,
    TransferTooLarge,
    TransferDigestMismatch,
    UserConsentRequired,
    SubscriptionUnavailable,
    RoamingBlocked,
    PolicySegmentLimit,
}

impl fmt::Display for SmsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "SMS segment is truncated"),
            Self::WrongMagic => write!(f, "SMS segment has wrong magic"),
            Self::WrongVersion(value) => {
                write!(f, "unsupported SMS segment version {value}")
            }
            Self::EmptyTransfer => write!(f, "SMS transfer is empty"),
            Self::EmptySegment => write!(f, "SMS segment payload is empty"),
            Self::PayloadTooLarge => write!(f, "SMS segment payload exceeds budget"),
            Self::TooManySegments => write!(f, "SMS transfer has too many segments"),
            Self::InvalidSegmentIndex => write!(f, "SMS segment index/count is invalid"),
            Self::TrailingBytes => write!(f, "SMS segment contains trailing bytes"),
            Self::TransferMismatch => write!(f, "SMS segment belongs to another transfer"),
            Self::ConflictingDuplicate => write!(f, "SMS segment conflicts with an existing index"),
            Self::TransferTooLarge => write!(f, "SMS reassembled transfer exceeds bound"),
            Self::TransferDigestMismatch => write!(f, "SMS reassembled transfer fingerprint mismatch"),
            Self::UserConsentRequired => write!(f, "SMS send requires explicit user consent"),
            Self::SubscriptionUnavailable => write!(f, "SMS subscription is unavailable"),
            Self::RoamingBlocked => write!(f, "SMS send blocked while roaming"),
            Self::PolicySegmentLimit => write!(f, "SMS send exceeds user/policy segment limit"),
        }
    }
}

impl std::error::Error for SmsError {}

fn validate_segment(segment: &SmsSegment) -> Result<(), SmsError> {
    if segment.payload.is_empty() {
        return Err(SmsError::EmptySegment);
    }
    if segment.payload.len() > SEGMENT_PAYLOAD_BYTES {
        return Err(SmsError::PayloadTooLarge);
    }
    if segment.count == 0 || segment.index >= segment.count {
        return Err(SmsError::InvalidSegmentIndex);
    }
    Ok(())
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

    fn take(&mut self, len: usize) -> Result<&'a [u8], SmsError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(SmsError::Truncated)?;
        if end > self.bytes.len() {
            return Err(SmsError::Truncated);
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, SmsError> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, SmsError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| SmsError::Truncated)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bytes() -> Vec<u8> {
        (0..235).map(|index| (index % 251) as u8).collect()
    }

    #[test]
    fn segmented_transfer_roundtrips_out_of_order() {
        let bytes = sample_bytes();
        let mut segments = segment_capsule(&bytes).unwrap();
        assert_eq!(segments.len(), 3);
        segments.reverse();

        let mut assembler = SmsAssembler::conservative();
        let mut completed = None;
        for segment in segments {
            if let AcceptOutcome::Complete(value) =
                assembler.accept(segment).unwrap()
            {
                completed = Some(value);
            }
        }

        assert_eq!(completed.unwrap(), bytes);
    }

    #[test]
    fn exact_duplicate_is_idempotent_and_conflicting_duplicate_fails() {
        let bytes = sample_bytes();
        let segments = segment_capsule(&bytes).unwrap();
        let first = segments[0].clone();

        let mut assembler = SmsAssembler::conservative();
        assert_eq!(
            assembler.accept(first.clone()).unwrap(),
            AcceptOutcome::Incomplete,
        );
        assert_eq!(
            assembler.accept(first.clone()).unwrap(),
            AcceptOutcome::Duplicate,
        );

        let mut conflicting = first;
        conflicting.payload[0] ^= 1;
        assert_eq!(
            assembler.accept(conflicting).unwrap_err(),
            SmsError::ConflictingDuplicate,
        );
    }

    #[test]
    fn segment_wire_never_exceeds_budget() {
        for segment in segment_capsule(&sample_bytes()).unwrap() {
            let wire = segment.encode().unwrap();
            assert!(wire.len() <= SMS_DATA_BUDGET_BYTES);
            assert_eq!(SmsSegment::decode(&wire).unwrap(), segment);
        }
    }

    #[test]
    fn policy_requires_consent_subscription_and_roaming_permission() {
        let mut policy = SmsSendPolicy {
            user_consented: false,
            subscription_available: true,
            roaming: false,
            allow_roaming: false,
            max_segments: 4,
        };
        assert_eq!(
            policy.authorize(1),
            Err(SmsError::UserConsentRequired),
        );

        policy.user_consented = true;
        policy.subscription_available = false;
        assert_eq!(
            policy.authorize(1),
            Err(SmsError::SubscriptionUnavailable),
        );

        policy.subscription_available = true;
        policy.roaming = true;
        assert_eq!(
            policy.authorize(1),
            Err(SmsError::RoamingBlocked),
        );

        policy.allow_roaming = true;
        assert_eq!(policy.authorize(4), Ok(()));
        assert_eq!(
            policy.authorize(5),
            Err(SmsError::PolicySegmentLimit),
        );
    }

    #[test]
    fn reassembled_digest_detects_corrupted_transfer() {
        let bytes = sample_bytes();
        let mut segments = segment_capsule(&bytes).unwrap();
        segments[1].payload[0] ^= 1;

        let mut assembler = SmsAssembler::conservative();
        let mut error = None;
        for segment in segments {
            match assembler.accept(segment) {
                Ok(_) => {}
                Err(value) => {
                    error = Some(value);
                    break;
                }
            }
        }
        assert_eq!(error, Some(SmsError::TransferDigestMismatch));
    }
}
