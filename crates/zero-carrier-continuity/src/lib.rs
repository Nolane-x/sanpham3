use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuityFreshness {
    /// Remote bytes were received earlier and are now being served locally.
    CachedRemote,
    /// Local deterministic/generated result. Never remote truth.
    LocallyGenerated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReceipt {
    pub source_id: String,
    pub observed_at_ms: u64,
    pub content_sha256: [u8; 32],
    pub provenance_note: String,
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
        }
    }

    pub fn verify(&self, bytes: &[u8]) -> bool {
        Sha256::digest(bytes).as_slice() == self.content_sha256
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

#[derive(Debug, Default)]
pub struct ContinuityStore {
    entries: HashMap<String, CachedObject>,
}

impl ContinuityStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_verified(
        &mut self,
        object: CachedObject,
    ) -> Result<(), &'static str> {
        if !object.receipt.verify(&object.bytes) {
            return Err("cached object does not match source receipt hash");
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

            if caller_accepts_age {
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
