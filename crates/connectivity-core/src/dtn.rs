use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BundlePriority {
    Bulk,
    Normal,
    Urgent,
}

#[derive(Debug, Clone)]
pub struct Bundle {
    pub id: u64,
    pub priority: BundlePriority,
    pub created_at: Instant,
    pub ttl: Duration,
    pub payload: Vec<u8>,
    pub attempts: u32,
}

impl Bundle {
    pub fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.created_at) >= self.ttl
    }
}

#[derive(Default)]
pub struct DtnQueue {
    bundles: VecDeque<Bundle>,
}

impl DtnQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bundle: Bundle) {
        self.bundles.push_back(bundle);
    }

    /// Inserts a bundle only when its ID is not already present.
    ///
    /// Bundle IDs are the DTN deduplication key. Returning false means the
    /// queue already has custody of an equivalent logical bundle ID.
    pub fn push_unique(&mut self, bundle: Bundle) -> bool {
        if self.contains(bundle.id) {
            return false;
        }

        self.bundles.push_back(bundle);
        true
    }

    pub fn contains(&self, id: u64) -> bool {
        self.bundles.iter().any(|bundle| bundle.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Bundle> {
        self.bundles.iter()
    }

    pub fn len(&self) -> usize {
        self.bundles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bundles.is_empty()
    }

    pub fn purge_expired(&mut self, now: Instant) -> usize {
        let before = self.bundles.len();
        self.bundles.retain(|bundle| !bundle.expired(now));
        before - self.bundles.len()
    }

    /// Returns the highest-priority live bundle and records a send attempt.
    pub fn next_for_send(&mut self, now: Instant) -> Option<&mut Bundle> {
        self.purge_expired(now);

        let index = self
            .bundles
            .iter()
            .enumerate()
            .max_by_key(|(_, bundle)| bundle.priority)
            .map(|(index, _)| index)?;

        let bundle = self.bundles.get_mut(index)?;
        bundle.attempts = bundle.attempts.saturating_add(1);
        Some(bundle)
    }

    pub fn ack(&mut self, id: u64) -> bool {
        let Some(index) = self.bundles.iter().position(|bundle| bundle.id == id)
        else {
            return false;
        };

        self.bundles.remove(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urgent_bundle_is_selected_first() {
        let now = Instant::now();
        let mut queue = DtnQueue::new();

        queue.push(Bundle {
            id: 1,
            priority: BundlePriority::Bulk,
            created_at: now,
            ttl: Duration::from_secs(60),
            payload: vec![1],
            attempts: 0,
        });

        queue.push(Bundle {
            id: 2,
            priority: BundlePriority::Urgent,
            created_at: now,
            ttl: Duration::from_secs(60),
            payload: vec![2],
            attempts: 0,
        });

        assert_eq!(queue.next_for_send(now).unwrap().id, 2);
    }


    #[test]
    fn duplicate_bundle_id_is_rejected_by_unique_insert() {
        let now = Instant::now();
        let mut queue = DtnQueue::new();

        let first = Bundle {
            id: 44,
            priority: BundlePriority::Normal,
            created_at: now,
            ttl: Duration::from_secs(60),
            payload: vec![1, 2, 3],
            attempts: 0,
        };
        let duplicate = Bundle {
            id: 44,
            priority: BundlePriority::Urgent,
            created_at: now,
            ttl: Duration::from_secs(120),
            payload: vec![9],
            attempts: 0,
        };

        assert!(queue.push_unique(first));
        assert!(!queue.push_unique(duplicate));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.iter().next().unwrap().payload, vec![1, 2, 3]);
    }

    #[test]
    fn expired_bundle_is_purged() {
        let created_at = Instant::now();
        let mut queue = DtnQueue::new();

        queue.push(Bundle {
            id: 1,
            priority: BundlePriority::Normal,
            created_at,
            ttl: Duration::from_millis(1),
            payload: vec![1],
            attempts: 0,
        });

        let later = created_at + Duration::from_secs(1);
        assert_eq!(queue.purge_expired(later), 1);
        assert!(queue.is_empty());
    }
}
