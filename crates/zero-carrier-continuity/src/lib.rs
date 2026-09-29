use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

const STORE_MAGIC: [u8; 4] = *b"SP3S";
const STORE_VERSION_UNSIGNED: u8 = 1;
const STORE_VERSION: u8 = 2;
const RECEIPT_SIGNATURE_DOMAIN: &[u8] = b"SP3-SOURCE-RECEIPT-V1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuityFreshness {
    /// Remote bytes were received earlier and are now being served locally.
    CachedRemote,
    /// Local deterministic/generated result. Never remote truth.
    LocallyGenerated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptSignature {
    pub verifying_key: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptSignatureError {
    MissingSignature,
    InvalidVerifyingKey,
    InvalidSignature,
    ContentHashMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReceipt {
    pub source_id: String,
    pub observed_at_ms: u64,
    pub content_sha256: [u8; 32],
    pub provenance_note: String,
    pub signature: Option<ReceiptSignature>,
}

impl SourceReceipt {
    pub fn for_bytes(
        source_id: impl Into<String>,
        observed_at_ms: u64,
        bytes: &[u8],
        provenance_note: impl Into<String>,
    ) -> Self {
        let digest = Sha256::digest(bytes);
        let mut content_sha256 = [0_u8; 32];
        content_sha256.copy_from_slice(&digest);

        Self {
            source_id: source_id.into(),
            observed_at_ms,
            content_sha256,
            provenance_note: provenance_note.into(),
            signature: None,
        }
    }

    pub fn signed_for_bytes(
        source_id: impl Into<String>,
        observed_at_ms: u64,
        bytes: &[u8],
        provenance_note: impl Into<String>,
        signing_key: &SigningKey,
    ) -> Self {
        let mut receipt = Self::for_bytes(
            source_id,
            observed_at_ms,
            bytes,
            provenance_note,
        );
        receipt.sign(signing_key);
        receipt
    }

    pub fn sign(&mut self, signing_key: &SigningKey) {
        let message = self.signing_message();
        let signature = signing_key.sign(&message).to_bytes();
        self.signature = Some(ReceiptSignature {
            verifying_key: signing_key.verifying_key().to_bytes(),
            signature,
        });
    }

    pub fn verify(&self, bytes: &[u8]) -> bool {
        Sha256::digest(bytes).as_slice() == self.content_sha256
    }

    pub fn verify_signature(&self) -> Result<(), ReceiptSignatureError> {
        let Some(signed) = &self.signature else {
            return Err(ReceiptSignatureError::MissingSignature);
        };
        let verifying_key = VerifyingKey::from_bytes(&signed.verifying_key)
            .map_err(|_| ReceiptSignatureError::InvalidVerifyingKey)?;
        let signature = Signature::from_bytes(&signed.signature);
        verifying_key
            .verify_strict(&self.signing_message(), &signature)
            .map_err(|_| ReceiptSignatureError::InvalidSignature)
    }

    pub fn verify_signed_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<(), ReceiptSignatureError> {
        if !self.verify(bytes) {
            return Err(ReceiptSignatureError::ContentHashMismatch);
        }
        self.verify_signature()
    }

    fn signing_message(&self) -> Vec<u8> {
        let source = self.source_id.as_bytes();
        let note = self.provenance_note.as_bytes();
        let source_len = u64::try_from(source.len()).unwrap_or(u64::MAX);
        let note_len = u64::try_from(note.len()).unwrap_or(u64::MAX);

        let mut message = Vec::with_capacity(
            RECEIPT_SIGNATURE_DOMAIN.len()
                + 8
                + source.len()
                + 8
                + 32
                + 8
                + note.len(),
        );
        message.extend_from_slice(RECEIPT_SIGNATURE_DOMAIN);
        message.extend_from_slice(&source_len.to_be_bytes());
        message.extend_from_slice(source);
        message.extend_from_slice(&self.observed_at_ms.to_be_bytes());
        message.extend_from_slice(&self.content_sha256);
        message.extend_from_slice(&note_len.to_be_bytes());
        message.extend_from_slice(note);
        message
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedObject {
    pub key: String,
    pub bytes: Vec<u8>,
    pub receipt: SourceReceipt,
    /// Source-supplied or adapter-defined validity horizon.
    pub valid_for: Duration,
}

impl CachedObject {
    pub fn age_at(&self, now_ms: u64) -> Duration {
        Duration::from_millis(
            now_ms.saturating_sub(self.receipt.observed_at_ms),
        )
    }

    pub fn within_source_validity(&self, now_ms: u64) -> bool {
        self.age_at(now_ms) <= self.valid_for
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuityContract {
    /// Caller refuses any answer that was not obtained over a current remote
    /// information path.
    pub require_current_remote_observation: bool,
    /// Maximum age caller accepts for cached remote data.
    pub max_cache_age: Option<Duration>,
    /// Whether a local deterministic/generated fallback is admissible.
    pub allow_generated: bool,
}

impl ContinuityContract {
    pub fn cached_ok(max_cache_age: Duration) -> Self {
        Self {
            require_current_remote_observation: false,
            max_cache_age: Some(max_cache_age),
            allow_generated: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuityAnswer {
    pub key: String,
    pub bytes: Vec<u8>,
    pub freshness: ContinuityFreshness,
    pub age: Option<Duration>,
    pub source_receipt: Option<SourceReceipt>,
    pub explanation: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuityMiss {
    CurrentRemoteRequired,
    NoAdmissibleCache,
    GenerationDisallowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconciliationPolicy {
    pub max_age: Duration,
    pub min_distinct_sources: usize,
    pub require_valid_signature: bool,
}

impl ReconciliationPolicy {
    pub fn quorum(
        max_age: Duration,
        min_distinct_sources: usize,
        require_valid_signature: bool,
    ) -> Self {
        Self {
            max_age,
            min_distinct_sources,
            require_valid_signature,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationError {
    InvalidPolicy,
    NoAdmissibleCandidates,
    InsufficientConsensus {
        required_sources: usize,
        best_support: usize,
    },
    Conflict {
        support_per_value: usize,
        competing_values: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciledObservation {
    pub bytes: Vec<u8>,
    pub content_sha256: [u8; 32],
    pub agreeing_sources: Vec<String>,
    pub oldest_observed_at_ms: u64,
    pub newest_observed_at_ms: u64,
    pub freshness: ContinuityFreshness,
}

/// Reconciles multiple cached observations of the same logical remote value.
///
/// The baseline is intentionally exact and conservative:
/// - invalid hashes, stale/source-invalid entries and (optionally) unsigned or
///   invalid signatures are excluded;
/// - one source ID contributes at most one vote;
/// - if one source presents different digests, that source is excluded for
///   this reconciliation;
/// - values are grouped by exact SHA-256 digest;
/// - a unique highest-support digest must meet the configured quorum;
/// - equal top support for different digests is an explicit conflict.
pub fn reconcile_cached_observations(
    candidates: &[CachedObject],
    now_ms: u64,
    policy: ReconciliationPolicy,
) -> Result<ReconciledObservation, ReconciliationError> {
    if policy.min_distinct_sources == 0 {
        return Err(ReconciliationError::InvalidPolicy);
    }

    let mut source_choice = HashMap::<String, &CachedObject>::new();
    let mut conflicted_sources = HashSet::<String>::new();

    for candidate in candidates {
        if !candidate.receipt.verify(&candidate.bytes)
            || !candidate.within_source_validity(now_ms)
            || candidate.age_at(now_ms) > policy.max_age
        {
            continue;
        }

        if policy.require_valid_signature
            && candidate.receipt.verify_signature().is_err()
        {
            continue;
        }

        let source = candidate.receipt.source_id.clone();
        if conflicted_sources.contains(&source) {
            continue;
        }

        match source_choice.get(&source).copied() {
            None => {
                source_choice.insert(source, candidate);
            }
            Some(existing)
                if existing.receipt.content_sha256
                    == candidate.receipt.content_sha256 =>
            {
                if candidate.receipt.observed_at_ms
                    > existing.receipt.observed_at_ms
                {
                    source_choice.insert(source, candidate);
                }
            }
            Some(_) => {
                source_choice.remove(&source);
                conflicted_sources.insert(source);
            }
        }
    }

    if source_choice.is_empty() {
        return Err(ReconciliationError::NoAdmissibleCandidates);
    }

    #[derive(Debug)]
    struct Group {
        bytes: Vec<u8>,
        sources: Vec<String>,
        oldest_observed_at_ms: u64,
        newest_observed_at_ms: u64,
    }

    let mut groups = HashMap::<[u8; 32], Group>::new();
    for (source, candidate) in source_choice {
        let digest = candidate.receipt.content_sha256;
        let observed = candidate.receipt.observed_at_ms;
        let group = groups.entry(digest).or_insert_with(|| Group {
            bytes: candidate.bytes.clone(),
            sources: Vec::new(),
            oldest_observed_at_ms: observed,
            newest_observed_at_ms: observed,
        });
        group.sources.push(source);
        group.oldest_observed_at_ms =
            group.oldest_observed_at_ms.min(observed);
        group.newest_observed_at_ms =
            group.newest_observed_at_ms.max(observed);
    }

    let best_support = groups
        .values()
        .map(|group| group.sources.len())
        .max()
        .unwrap_or(0);

    if best_support < policy.min_distinct_sources {
        return Err(ReconciliationError::InsufficientConsensus {
            required_sources: policy.min_distinct_sources,
            best_support,
        });
    }

    let competing_values = groups
        .values()
        .filter(|group| group.sources.len() == best_support)
        .count();

    if competing_values > 1 {
        return Err(ReconciliationError::Conflict {
            support_per_value: best_support,
            competing_values,
        });
    }

    let (content_sha256, mut group) = groups
        .into_iter()
        .find(|(_, group)| group.sources.len() == best_support)
        .ok_or(ReconciliationError::NoAdmissibleCandidates)?;

    group.sources.sort();

    Ok(ReconciledObservation {
        bytes: group.bytes,
        content_sha256,
        agreeing_sources: group.sources,
        oldest_observed_at_ms: group.oldest_observed_at_ms,
        newest_observed_at_ms: group.newest_observed_at_ms,
        freshness: ContinuityFreshness::CachedRemote,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistenceLimits {
    pub max_store_bytes: u64,
    pub max_entries: usize,
    pub max_object_bytes: u64,
}

impl PersistenceLimits {
    pub fn conservative() -> Self {
        Self {
            max_store_bytes: 64 * 1024 * 1024,
            max_entries: 10_000,
            max_object_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotReport {
    pub entries: usize,
    pub snapshot_bytes: u64,
    pub snapshot_sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionReport {
    pub entries_before: usize,
    pub entries_after: usize,
    pub payload_bytes_before: u64,
    pub payload_bytes_after: u64,
    pub removed_keys: Vec<String>,
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(io::Error),
    InvalidFormat(&'static str),
    ResourceLimit(&'static str),
    InvalidUtf8,
    ReceiptMismatch,
    InvalidSourceSignature,
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "continuity persistence I/O: {error}"),
            Self::InvalidFormat(message) => {
                write!(f, "invalid continuity snapshot: {message}")
            }
            Self::ResourceLimit(message) => {
                write!(f, "continuity snapshot resource limit: {message}")
            }
            Self::InvalidUtf8 => write!(f, "continuity snapshot contains invalid UTF-8"),
            Self::ReceiptMismatch => {
                write!(f, "continuity snapshot object does not match receipt hash")
            }
            Self::InvalidSourceSignature => {
                write!(f, "continuity snapshot source receipt signature is invalid")
            }
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<io::Error> for PersistenceError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Default)]
pub struct ContinuityStore {
    entries: HashMap<String, CachedObject>,
}

impl ContinuityStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn payload_bytes(&self) -> u64 {
        self.entries
            .values()
            .map(|object| object.bytes.len() as u64)
            .sum()
    }

    /// Removes cache entries until both entry-count and payload-byte budgets
    /// are satisfied.
    ///
    /// Eviction is deterministic:
    /// 1. source-invalid entries first;
    /// 2. then older remote observations;
    /// 3. then lexical key order.
    pub fn evict_to_budget(
        &mut self,
        now_ms: u64,
        max_payload_bytes: u64,
        max_entries: usize,
    ) -> EvictionReport {
        let entries_before = self.entry_count();
        let payload_bytes_before = self.payload_bytes();
        let mut candidates = self
            .entries
            .values()
            .map(|object| {
                (
                    !object.within_source_validity(now_ms),
                    object.receipt.observed_at_ms,
                    object.key.clone(),
                )
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| {
            // Invalid entries sort first.
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });

        let mut removed_keys = Vec::new();
        let mut payload_bytes = payload_bytes_before;

        for (_, _, key) in candidates {
            if self.entries.len() <= max_entries
                && payload_bytes <= max_payload_bytes
            {
                break;
            }

            if let Some(removed) = self.entries.remove(&key) {
                payload_bytes =
                    payload_bytes.saturating_sub(removed.bytes.len() as u64);
                removed_keys.push(key);
            }
        }

        EvictionReport {
            entries_before,
            entries_after: self.entry_count(),
            payload_bytes_before,
            payload_bytes_after: self.payload_bytes(),
            removed_keys,
        }
    }

    /// Persists a deterministic versioned snapshot.
    ///
    /// The implementation writes and syncs a sibling temporary file before
    /// replacement. On platforms where replacing an existing destination via
    /// rename is unavailable, it falls back to remove+rename; therefore this
    /// is a durable snapshot baseline, not a universal crash-atomic claim.
    pub fn save_snapshot(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<SnapshotReport, PersistenceError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut entries = self.entries.values().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.key.cmp(&right.key));

        let count = u32::try_from(entries.len())
            .map_err(|_| PersistenceError::ResourceLimit("too many entries"))?;

        let mut snapshot = Vec::new();
        snapshot.extend_from_slice(&STORE_MAGIC);
        snapshot.push(STORE_VERSION);
        snapshot.extend_from_slice(&count.to_be_bytes());

        for object in entries {
            if !object.receipt.verify(&object.bytes) {
                return Err(PersistenceError::ReceiptMismatch);
            }
            if object.receipt.signature.is_some()
                && object.receipt.verify_signature().is_err()
            {
                return Err(PersistenceError::InvalidSourceSignature);
            }

            let key = object.key.as_bytes();
            let source = object.receipt.source_id.as_bytes();
            let note = object.receipt.provenance_note.as_bytes();
            let key_len = u32::try_from(key.len())
                .map_err(|_| PersistenceError::ResourceLimit("key too large"))?;
            let source_len = u32::try_from(source.len())
                .map_err(|_| PersistenceError::ResourceLimit("source ID too large"))?;
            let note_len = u32::try_from(note.len())
                .map_err(|_| PersistenceError::ResourceLimit("provenance note too large"))?;
            let bytes_len = u64::try_from(object.bytes.len())
                .map_err(|_| PersistenceError::ResourceLimit("object too large"))?;
            let valid_for_ms = u64::try_from(object.valid_for.as_millis())
                .map_err(|_| PersistenceError::ResourceLimit("validity horizon too large"))?;

            snapshot.extend_from_slice(&key_len.to_be_bytes());
            snapshot.extend_from_slice(&source_len.to_be_bytes());
            snapshot.extend_from_slice(&note_len.to_be_bytes());
            snapshot.extend_from_slice(&bytes_len.to_be_bytes());
            snapshot.extend_from_slice(&object.receipt.observed_at_ms.to_be_bytes());
            snapshot.extend_from_slice(&valid_for_ms.to_be_bytes());
            snapshot.extend_from_slice(&object.receipt.content_sha256);
            match &object.receipt.signature {
                None => snapshot.push(0),
                Some(signed) => {
                    snapshot.push(1);
                    snapshot.extend_from_slice(&signed.verifying_key);
                    snapshot.extend_from_slice(&signed.signature);
                }
            }
            snapshot.extend_from_slice(key);
            snapshot.extend_from_slice(source);
            snapshot.extend_from_slice(note);
            snapshot.extend_from_slice(&object.bytes);
        }

        let snapshot_sha256: [u8; 32] = Sha256::digest(&snapshot).into();
        let temp_path = path.with_extension("sp3s.tmp");

        {
            let mut file = File::create(&temp_path)?;
            file.write_all(&snapshot)?;
            file.flush()?;
            file.sync_all()?;
        }

        match fs::rename(&temp_path, path) {
            Ok(()) => {}
            Err(error)
                if path.exists()
                    && matches!(
                        error.kind(),
                        io::ErrorKind::AlreadyExists
                            | io::ErrorKind::PermissionDenied
                    ) =>
            {
                fs::remove_file(path)?;
                fs::rename(&temp_path, path)?;
            }
            Err(error) => return Err(PersistenceError::Io(error)),
        }

        Ok(SnapshotReport {
            entries: self.entry_count(),
            snapshot_bytes: snapshot.len() as u64,
            snapshot_sha256,
        })
    }

    pub fn load_snapshot(
        path: impl AsRef<Path>,
        limits: PersistenceLimits,
    ) -> Result<Self, PersistenceError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path)?;
        if metadata.len() > limits.max_store_bytes {
            return Err(PersistenceError::ResourceLimit(
                "snapshot exceeds max_store_bytes",
            ));
        }

        let capacity = usize::try_from(metadata.len())
            .map_err(|_| PersistenceError::ResourceLimit("snapshot too large"))?;
        let mut snapshot = Vec::with_capacity(capacity);
        File::open(path)?.read_to_end(&mut snapshot)?;

        let mut cursor = StoreCursor::new(&snapshot);
        if cursor.take(4)? != STORE_MAGIC {
            return Err(PersistenceError::InvalidFormat("wrong snapshot magic"));
        }
        let version = cursor.u8()?;
        if version != STORE_VERSION_UNSIGNED && version != STORE_VERSION {
            return Err(PersistenceError::InvalidFormat(
                "unsupported snapshot version",
            ));
        }

        let entry_count = cursor.u32()? as usize;
        if entry_count > limits.max_entries {
            return Err(PersistenceError::ResourceLimit(
                "snapshot exceeds max_entries",
            ));
        }

        let mut store = ContinuityStore::new();
        for _ in 0..entry_count {
            let key_len = cursor.u32()? as usize;
            let source_len = cursor.u32()? as usize;
            let note_len = cursor.u32()? as usize;
            let bytes_len_u64 = cursor.u64()?;
            if bytes_len_u64 > limits.max_object_bytes {
                return Err(PersistenceError::ResourceLimit(
                    "object exceeds max_object_bytes",
                ));
            }
            let bytes_len = usize::try_from(bytes_len_u64)
                .map_err(|_| PersistenceError::ResourceLimit("object too large"))?;
            let observed_at_ms = cursor.u64()?;
            let valid_for_ms = cursor.u64()?;
            let content_sha256 = cursor.array32()?;
            let signature = if version >= STORE_VERSION {
                match cursor.u8()? {
                    0 => None,
                    1 => Some(ReceiptSignature {
                        verifying_key: cursor.array32()?,
                        signature: cursor.array64()?,
                    }),
                    _ => {
                        return Err(PersistenceError::InvalidFormat(
                            "invalid receipt signature flag",
                        ))
                    }
                }
            } else {
                None
            };

            let key = decode_utf8(cursor.take(key_len)?)?;
            let source_id = decode_utf8(cursor.take(source_len)?)?;
            let provenance_note = decode_utf8(cursor.take(note_len)?)?;
            let bytes = cursor.take(bytes_len)?.to_vec();

            if store.entries.contains_key(&key) {
                return Err(PersistenceError::InvalidFormat(
                    "duplicate cache key",
                ));
            }

            let object = CachedObject {
                key: key.clone(),
                bytes,
                receipt: SourceReceipt {
                    source_id,
                    observed_at_ms,
                    content_sha256,
                    provenance_note,
                    signature,
                },
                valid_for: Duration::from_millis(valid_for_ms),
            };

            if !object.receipt.verify(&object.bytes) {
                return Err(PersistenceError::ReceiptMismatch);
            }
            if object.receipt.signature.is_some()
                && object.receipt.verify_signature().is_err()
            {
                return Err(PersistenceError::InvalidSourceSignature);
            }
            store.entries.insert(key, object);
        }

        if cursor.remaining() != 0 {
            return Err(PersistenceError::InvalidFormat(
                "trailing bytes after snapshot",
            ));
        }

        Ok(store)
    }

    pub fn insert_verified(
        &mut self,
        object: CachedObject,
    ) -> Result<(), &'static str> {
        if !object.receipt.verify(&object.bytes) {
            return Err("cached object does not match source receipt hash");
        }
        if object.receipt.signature.is_some()
            && object.receipt.verify_signature().is_err()
        {
            return Err("cached object source receipt signature is invalid");
        }
        self.entries.insert(object.key.clone(), object);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&CachedObject> {
        self.entries.get(key)
    }

    /// Resolves a request when there is currently no remote information path.
    ///
    /// This function intentionally has no variant that returns FreshRemote.
    /// A current remote observation must come from a separate live-carrier
    /// pipeline.
    pub fn resolve_zero_carrier<F>(
        &self,
        key: &str,
        now_ms: u64,
        contract: ContinuityContract,
        generator: Option<F>,
    ) -> Result<ContinuityAnswer, ContinuityMiss>
    where
        F: FnOnce(&str) -> Vec<u8>,
    {
        if contract.require_current_remote_observation {
            return Err(ContinuityMiss::CurrentRemoteRequired);
        }

        if let Some(cached) = self.entries.get(key) {
            let age = cached.age_at(now_ms);
            let caller_accepts_age = contract
                .max_cache_age
                .is_some_and(|limit| age <= limit);

            if caller_accepts_age && cached.within_source_validity(now_ms) {
                return Ok(ContinuityAnswer {
                    key: key.to_owned(),
                    bytes: cached.bytes.clone(),
                    freshness: ContinuityFreshness::CachedRemote,
                    age: Some(age),
                    source_receipt: Some(cached.receipt.clone()),
                    explanation:
                        "served locally from previously observed remote bytes",
                });
            }
        }

        if !contract.allow_generated {
            return Err(if self.entries.contains_key(key) {
                ContinuityMiss::NoAdmissibleCache
            } else {
                ContinuityMiss::GenerationDisallowed
            });
        }

        let Some(generator) = generator else {
            return Err(ContinuityMiss::GenerationDisallowed);
        };
        let bytes = generator(key);

        Ok(ContinuityAnswer {
            key: key.to_owned(),
            bytes,
            freshness: ContinuityFreshness::LocallyGenerated,
            age: None,
            source_receipt: None,
            explanation:
                "locally generated fallback; not a remote observation",
        })
    }
}

fn decode_utf8(bytes: &[u8]) -> Result<String, PersistenceError> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| PersistenceError::InvalidUtf8)
}

struct StoreCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> StoreCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], PersistenceError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(PersistenceError::InvalidFormat("cursor overflow"))?;
        if end > self.bytes.len() {
            return Err(PersistenceError::InvalidFormat("truncated snapshot"));
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, PersistenceError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, PersistenceError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| PersistenceError::InvalidFormat("truncated u32"))?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, PersistenceError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| PersistenceError::InvalidFormat("truncated u64"))?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn array32(&mut self) -> Result<[u8; 32], PersistenceError> {
        self.take(32)?
            .try_into()
            .map_err(|_| PersistenceError::InvalidFormat("truncated digest"))
    }

    fn array64(&mut self) -> Result<[u8; 64], PersistenceError> {
        self.take(64)?
            .try_into()
            .map_err(|_| PersistenceError::InvalidFormat("truncated signature"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTwinRecipe {
    pub recipe_id: String,
    pub required_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTwinAnswer {
    pub recipe_id: String,
    pub bytes: Vec<u8>,
    pub inputs: Vec<SourceReceipt>,
    pub oldest_input_age: Duration,
    pub freshness: ContinuityFreshness,
}

/// Executes a deterministic local service twin using only cached remote inputs.
///
/// The twin result inherits CachedRemote provenance. It never becomes
/// FreshRemote merely because the transformation itself is current.
pub fn run_service_twin<F>(
    store: &ContinuityStore,
    recipe: &ServiceTwinRecipe,
    now_ms: u64,
    max_input_age: Duration,
    transform: F,
) -> Result<ServiceTwinAnswer, ContinuityMiss>
where
    F: FnOnce(&[(&str, &[u8])]) -> Vec<u8>,
{
    let mut inputs = Vec::with_capacity(recipe.required_keys.len());
    let mut receipts = Vec::with_capacity(recipe.required_keys.len());
    let mut oldest_age = Duration::ZERO;

    for key in &recipe.required_keys {
        let Some(object) = store.get(key) else {
            return Err(ContinuityMiss::NoAdmissibleCache);
        };

        let age = object.age_at(now_ms);
        if age > max_input_age {
            return Err(ContinuityMiss::NoAdmissibleCache);
        }

        oldest_age = oldest_age.max(age);
        inputs.push((key.as_str(), object.bytes.as_slice()));
        receipts.push(object.receipt.clone());
    }

    let bytes = transform(&inputs);

    Ok(ServiceTwinAnswer {
        recipe_id: recipe.recipe_id.clone(),
        bytes,
        inputs: receipts,
        oldest_input_age: oldest_age,
        freshness: ContinuityFreshness::CachedRemote,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(
        key: &str,
        source: &str,
        observed_at_ms: u64,
        value: &[u8],
    ) -> CachedObject {
        CachedObject {
            key: key.to_owned(),
            bytes: value.to_vec(),
            receipt: SourceReceipt::for_bytes(
                source,
                observed_at_ms,
                value,
                "test fixture",
            ),
            valid_for: Duration::from_secs(300),
        }
    }

    fn signed_object(
        key: &str,
        source: &str,
        observed_at_ms: u64,
        value: &[u8],
        signing_seed: u8,
    ) -> CachedObject {
        let signing_key = SigningKey::from_bytes(&[signing_seed; 32]);
        CachedObject {
            key: key.to_owned(),
            bytes: value.to_vec(),
            receipt: SourceReceipt::signed_for_bytes(
                source,
                observed_at_ms,
                value,
                "signed test fixture",
                &signing_key,
            ),
            valid_for: Duration::from_secs(300),
        }
    }

    #[test]
    fn current_remote_contract_cannot_be_satisfied_without_carrier() {
        let mut store = ContinuityStore::new();
        store
            .insert_verified(object("weather", "source-a", 1_000, b"sunny"))
            .unwrap();

        let result = store.resolve_zero_carrier::<fn(&str) -> Vec<u8>>(
            "weather",
            2_000,
            ContinuityContract {
                require_current_remote_observation: true,
                max_cache_age: Some(Duration::from_secs(60)),
                allow_generated: true,
            },
            None,
        );

        assert_eq!(result, Err(ContinuityMiss::CurrentRemoteRequired));
    }

    #[test]
    fn cached_answer_preserves_source_receipt_and_age() {
        let mut store = ContinuityStore::new();
        store
            .insert_verified(object("price", "market-source", 10_000, b"42"))
            .unwrap();

        let answer = store
            .resolve_zero_carrier::<fn(&str) -> Vec<u8>>(
                "price",
                25_000,
                ContinuityContract::cached_ok(Duration::from_secs(30)),
                None,
            )
            .unwrap();

        assert_eq!(answer.freshness, ContinuityFreshness::CachedRemote);
        assert_eq!(answer.age, Some(Duration::from_secs(15)));
        assert_eq!(
            answer.source_receipt.as_ref().unwrap().source_id,
            "market-source"
        );
        assert!(
            answer
                .source_receipt
                .as_ref()
                .unwrap()
                .verify(&answer.bytes)
        );
    }

    #[test]
    fn stale_cache_can_fall_back_to_explicit_local_generation() {
        let mut store = ContinuityStore::new();
        store
            .insert_verified(object("summary", "source-a", 0, b"old"))
            .unwrap();

        let answer = store
            .resolve_zero_carrier(
                "summary",
                120_000,
                ContinuityContract {
                    require_current_remote_observation: false,
                    max_cache_age: Some(Duration::from_secs(10)),
                    allow_generated: true,
                },
                Some(|key: &str| format!("generated:{key}").into_bytes()),
            )
            .unwrap();

        assert_eq!(
            answer.freshness,
            ContinuityFreshness::LocallyGenerated
        );
        assert_eq!(answer.source_receipt, None);
        assert_eq!(answer.bytes, b"generated:summary");
    }

    #[test]
    fn corrupted_cache_is_rejected_at_insert_boundary() {
        let mut corrupted = object("x", "source", 0, b"good");
        corrupted.bytes = b"tampered".to_vec();

        let mut store = ContinuityStore::new();
        assert!(store.insert_verified(corrupted).is_err());
    }

    fn temp_snapshot_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "sp3-continuity-{label}-{}-snapshot.bin",
            std::process::id(),
        ))
    }

    #[test]
    fn signed_source_receipt_verifies_content_and_metadata() {
        let signing_key = SigningKey::from_bytes(&[0x5A; 32]);
        let bytes = b"remote observation";
        let receipt = SourceReceipt::signed_for_bytes(
            "resolver-a",
            42_000,
            bytes,
            "peer-egress observation",
            &signing_key,
        );

        assert!(receipt.verify_signed_bytes(bytes).is_ok());

        let mut tampered_source = receipt.clone();
        tampered_source.source_id.push('x');
        assert_eq!(
            tampered_source.verify_signature(),
            Err(ReceiptSignatureError::InvalidSignature),
        );

        let mut tampered_time = receipt.clone();
        tampered_time.observed_at_ms += 1;
        assert_eq!(
            tampered_time.verify_signature(),
            Err(ReceiptSignatureError::InvalidSignature),
        );

        assert_eq!(
            receipt.verify_signed_bytes(b"different bytes"),
            Err(ReceiptSignatureError::ContentHashMismatch),
        );
    }

    #[test]
    fn insert_rejects_invalid_signed_metadata_even_when_content_hash_matches() {
        let signing_key = SigningKey::from_bytes(&[0x44; 32]);
        let bytes = b"signed object".to_vec();
        let mut receipt = SourceReceipt::signed_for_bytes(
            "source-a",
            1_000,
            &bytes,
            "original provenance",
            &signing_key,
        );
        receipt.provenance_note = "tampered provenance".to_owned();

        let mut store = ContinuityStore::new();
        assert_eq!(
            store.insert_verified(CachedObject {
                key: "signed".to_owned(),
                bytes,
                receipt,
                valid_for: Duration::from_secs(60),
            }),
            Err("cached object source receipt signature is invalid"),
        );
    }

    #[test]
    fn signed_receipt_survives_snapshot_restart() {
        let path = temp_snapshot_path("signed-roundtrip");
        let _ = fs::remove_file(&path);
        let signing_key = SigningKey::from_bytes(&[0x33; 32]);
        let bytes = b"signed cached remote bytes".to_vec();

        let mut store = ContinuityStore::new();
        store
            .insert_verified(CachedObject {
                key: "signed".to_owned(),
                bytes: bytes.clone(),
                receipt: SourceReceipt::signed_for_bytes(
                    "remote-signer",
                    77_000,
                    &bytes,
                    "signed provenance",
                    &signing_key,
                ),
                valid_for: Duration::from_secs(60),
            })
            .unwrap();

        store.save_snapshot(&path).unwrap();
        let loaded = ContinuityStore::load_snapshot(
            &path,
            PersistenceLimits::conservative(),
        )
        .unwrap();

        let object = loaded.get("signed").unwrap();
        assert!(object.receipt.verify_signed_bytes(&object.bytes).is_ok());
        assert_eq!(
            object
                .receipt
                .signature
                .as_ref()
                .unwrap()
                .verifying_key,
            signing_key.verifying_key().to_bytes(),
        );

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn persistent_snapshot_roundtrips_verified_receipts() {
        let path = temp_snapshot_path("roundtrip");
        let _ = fs::remove_file(&path);

        let mut store = ContinuityStore::new();
        store
            .insert_verified(object("weather", "remote-a", 10_000, b"sunny"))
            .unwrap();
        store
            .insert_verified(object("price", "remote-b", 11_000, b"42"))
            .unwrap();

        let report = store.save_snapshot(&path).unwrap();
        assert_eq!(report.entries, 2);
        assert!(report.snapshot_bytes > 0);

        let loaded = ContinuityStore::load_snapshot(
            &path,
            PersistenceLimits::conservative(),
        )
        .unwrap();

        assert_eq!(loaded.entry_count(), 2);
        assert_eq!(loaded.get("weather").unwrap().bytes, b"sunny");
        assert_eq!(
            loaded.get("weather").unwrap().receipt.source_id,
            "remote-a",
        );
        assert!(loaded
            .get("price")
            .unwrap()
            .receipt
            .verify(&loaded.get("price").unwrap().bytes));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn persistent_snapshot_rejects_content_tampering() {
        let path = temp_snapshot_path("tamper");
        let _ = fs::remove_file(&path);

        let mut store = ContinuityStore::new();
        store
            .insert_verified(object("weather", "remote-a", 10_000, b"sunny"))
            .unwrap();
        store.save_snapshot(&path).unwrap();

        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, bytes).unwrap();

        assert!(matches!(
            ContinuityStore::load_snapshot(
                &path,
                PersistenceLimits::conservative(),
            ),
            Err(PersistenceError::ReceiptMismatch),
        ));

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn eviction_prefers_source_invalid_then_oldest() {
        let mut store = ContinuityStore::new();
        let mut expired = object("expired", "source", 0, b"1111");
        expired.valid_for = Duration::from_secs(1);
        let mut oldest = object("oldest", "source", 2_000, b"2222");
        oldest.valid_for = Duration::from_secs(100);
        let mut newest = object("newest", "source", 3_000, b"3333");
        newest.valid_for = Duration::from_secs(100);

        store.insert_verified(expired).unwrap();
        store.insert_verified(oldest).unwrap();
        store.insert_verified(newest).unwrap();

        let first = store.evict_to_budget(5_000, 8, 2);
        assert_eq!(first.removed_keys, vec!["expired".to_owned()]);
        assert!(store.get("expired").is_none());

        let second = store.evict_to_budget(5_000, 4, 10);
        assert_eq!(second.removed_keys, vec!["oldest".to_owned()]);
        assert!(store.get("newest").is_some());
    }

    #[test]
    fn source_validity_is_stricter_than_caller_cache_age() {
        let mut store = ContinuityStore::new();
        let mut cached = object("weather", "source", 0, b"sunny");
        cached.valid_for = Duration::from_secs(5);
        store.insert_verified(cached).unwrap();

        let result = store.resolve_zero_carrier::<fn(&str) -> Vec<u8>>(
            "weather",
            10_000,
            ContinuityContract::cached_ok(Duration::from_secs(60)),
            None,
        );

        assert_eq!(result, Err(ContinuityMiss::NoAdmissibleCache));
    }

    #[test]
    fn multisource_quorum_selects_unique_exact_majority() {
        let candidates = vec![
            signed_object("weather", "source-a", 10_000, b"sunny", 1),
            signed_object("weather", "source-b", 11_000, b"sunny", 2),
            signed_object("weather", "source-c", 12_000, b"rainy", 3),
        ];

        let reconciled = reconcile_cached_observations(
            &candidates,
            20_000,
            ReconciliationPolicy::quorum(
                Duration::from_secs(60),
                2,
                true,
            ),
        )
        .unwrap();

        assert_eq!(reconciled.bytes, b"sunny");
        assert_eq!(
            reconciled.agreeing_sources,
            vec!["source-a".to_owned(), "source-b".to_owned()],
        );
        assert_eq!(
            reconciled.freshness,
            ContinuityFreshness::CachedRemote,
        );
    }

    #[test]
    fn multisource_equal_top_support_is_explicit_conflict() {
        let candidates = vec![
            signed_object("weather", "source-a", 10_000, b"sunny", 1),
            signed_object("weather", "source-b", 10_000, b"rainy", 2),
        ];

        assert_eq!(
            reconcile_cached_observations(
                &candidates,
                20_000,
                ReconciliationPolicy::quorum(
                    Duration::from_secs(60),
                    1,
                    true,
                ),
            ),
            Err(ReconciliationError::Conflict {
                support_per_value: 1,
                competing_values: 2,
            }),
        );
    }

    #[test]
    fn repeated_observations_from_one_source_do_not_fake_quorum() {
        let candidates = vec![
            signed_object("weather", "source-a", 10_000, b"sunny", 1),
            signed_object("weather", "source-a", 11_000, b"sunny", 1),
        ];

        assert_eq!(
            reconcile_cached_observations(
                &candidates,
                20_000,
                ReconciliationPolicy::quorum(
                    Duration::from_secs(60),
                    2,
                    true,
                ),
            ),
            Err(ReconciliationError::InsufficientConsensus {
                required_sources: 2,
                best_support: 1,
            }),
        );
    }

    #[test]
    fn internally_conflicting_source_is_excluded_from_quorum() {
        let candidates = vec![
            signed_object("weather", "source-a", 10_000, b"sunny", 1),
            signed_object("weather", "source-a", 11_000, b"rainy", 1),
            signed_object("weather", "source-b", 12_000, b"sunny", 2),
            signed_object("weather", "source-c", 13_000, b"sunny", 3),
        ];

        let reconciled = reconcile_cached_observations(
            &candidates,
            20_000,
            ReconciliationPolicy::quorum(
                Duration::from_secs(60),
                2,
                true,
            ),
        )
        .unwrap();

        assert_eq!(reconciled.bytes, b"sunny");
        assert_eq!(
            reconciled.agreeing_sources,
            vec!["source-b".to_owned(), "source-c".to_owned()],
        );
    }

    #[test]
    fn signed_quorum_excludes_unsigned_and_stale_candidates() {
        let signed = signed_object(
            "weather",
            "source-a",
            50_000,
            b"sunny",
            1,
        );
        let unsigned = object("weather", "source-b", 50_000, b"sunny");
        let stale = signed_object(
            "weather",
            "source-c",
            0,
            b"sunny",
            3,
        );

        assert_eq!(
            reconcile_cached_observations(
                &[signed, unsigned, stale],
                60_000,
                ReconciliationPolicy::quorum(
                    Duration::from_secs(30),
                    2,
                    true,
                ),
            ),
            Err(ReconciliationError::InsufficientConsensus {
                required_sources: 2,
                best_support: 1,
            }),
        );
    }

    #[test]
    fn service_twin_inherits_cached_remote_provenance() {
        let mut store = ContinuityStore::new();
        store
            .insert_verified(object("temp", "weather", 10_000, b"30"))
            .unwrap();
        store
            .insert_verified(object("humidity", "weather", 11_000, b"70"))
            .unwrap();

        let twin = run_service_twin(
            &store,
            &ServiceTwinRecipe {
                recipe_id: "weather-card-v1".to_owned(),
                required_keys: vec![
                    "temp".to_owned(),
                    "humidity".to_owned(),
                ],
            },
            20_000,
            Duration::from_secs(30),
            |inputs| {
                format!(
                    "{}C {}%",
                    String::from_utf8_lossy(inputs[0].1),
                    String::from_utf8_lossy(inputs[1].1),
                )
                .into_bytes()
            },
        )
        .unwrap();

        assert_eq!(
            twin.freshness,
            ContinuityFreshness::CachedRemote
        );
        assert_eq!(twin.inputs.len(), 2);
        assert_eq!(twin.oldest_input_age, Duration::from_secs(10));
        assert_eq!(twin.bytes, b"30C 70%");
    }
}
