use fragment_transport::{
    fragment_for_wire_budget, FragmentError, FragmentKey, TransferDescriptor,
    TransferId,
};
use std::cmp::Ordering;
use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTransfer {
    pub transfer_id: TransferId,
    pub fresh_until: Duration,
    pub priority: u32,
    wires: Vec<Vec<u8>>,
    next_wire: usize,
}

impl PendingTransfer {
    pub fn from_bytes(
        bytes: &[u8],
        wire_budget: usize,
        key: &FragmentKey,
        fresh_until: Duration,
        priority: u32,
    ) -> Result<Self, ScheduleError> {
        let descriptor = TransferDescriptor::from_bytes(bytes);
        let wires = fragment_for_wire_budget(bytes, wire_budget, key)?;
        Ok(Self {
            transfer_id: descriptor.transfer_id,
            fresh_until,
            priority,
            wires,
            next_wire: 0,
        })
    }

    pub fn is_complete(&self) -> bool {
        self.next_wire >= self.wires.len()
    }

    pub fn remaining_wire_count(&self) -> usize {
        self.wires.len().saturating_sub(self.next_wire)
    }

    pub fn remaining_wire_bytes(&self) -> u64 {
        self.wires[self.next_wire..]
            .iter()
            .map(|wire| wire.len() as u64)
            .sum()
    }

    pub fn next_wire_bytes(&self) -> Option<usize> {
        self.wires.get(self.next_wire).map(Vec::len)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactBudget {
    pub starts_at: Duration,
    pub duration: Duration,
    pub bitrate_bps: u64,
    /// Optional measured/application cap after lower-layer overhead.
    pub max_wire_bytes: Option<u64>,
}

impl ContactBudget {
    pub fn wire_capacity_bytes(self) -> Result<u64, ScheduleError> {
        if self.bitrate_bps == 0 || self.duration.is_zero() {
            return Err(ScheduleError::ZeroCapacity);
        }

        let bits = u128::from(self.bitrate_bps)
            .saturating_mul(self.duration.as_nanos())
            / 1_000_000_000_u128;
        let bytes = (bits / 8).min(u128::from(u64::MAX)) as u64;
        let bytes = self.max_wire_bytes.map_or(bytes, |limit| bytes.min(limit));

        if bytes == 0 {
            return Err(ScheduleError::ZeroCapacity);
        }
        Ok(bytes)
    }

    pub fn ends_at(self) -> Duration {
        self.starts_at.saturating_add(self.duration)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledFragment {
    pub transfer_id: TransferId,
    pub wire_index: usize,
    pub wire: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactSchedule {
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub fragments: Vec<ScheduledFragment>,
    pub completed_transfers: Vec<TransferId>,
    pub expired_transfers: Vec<TransferId>,
}

impl ContactSchedule {
    pub fn unused_bytes(&self) -> u64 {
        self.capacity_bytes.saturating_sub(self.used_bytes)
    }
}

/// Schedules already-authenticated fragment wires into one contact.
///
/// Baseline policy:
/// 1. never spend capacity on a transfer already past its freshness deadline;
/// 2. prefer transfers that can still finish within this contact and before
///    their freshness deadline;
/// 3. then prefer the earlier freshness deadline;
/// 4. then higher application priority;
/// 5. then fewer remaining wire bytes.
///
/// If no transfer can finish in this contact, partial progress is still
/// allowed for non-expired transfers, ordered by the same deadline/priority
/// policy.
pub fn schedule_contact(
    transfers: &mut [PendingTransfer],
    contact: ContactBudget,
) -> Result<ContactSchedule, ScheduleError> {
    let capacity = contact.wire_capacity_bytes()?;
    let mut remaining_capacity = capacity;
    let mut fragments = Vec::new();
    let mut completed = Vec::new();
    let mut expired = transfers
        .iter()
        .filter(|transfer| {
            !transfer.is_complete() && transfer.fresh_until <= contact.starts_at
        })
        .map(|transfer| transfer.transfer_id)
        .collect::<Vec<_>>();
    expired.sort();

    loop {
        let used_bytes = capacity.saturating_sub(remaining_capacity);
        let current_time = contact.starts_at.saturating_add(
            serialization_duration(used_bytes, contact.bitrate_bps),
        );

        let mut candidates = transfers
            .iter()
            .enumerate()
            .filter(|(_, transfer)| {
                if transfer.is_complete() || transfer.fresh_until <= current_time {
                    return false;
                }

                let Some(next_wire_bytes) = transfer.next_wire_bytes() else {
                    return false;
                };
                if next_wire_bytes as u64 > remaining_capacity {
                    return false;
                }

                let next_finish = current_time.saturating_add(
                    serialization_duration(
                        next_wire_bytes as u64,
                        contact.bitrate_bps,
                    ),
                );

                next_finish <= contact.ends_at()
                    && next_finish <= transfer.fresh_until
            })
            .map(|(index, transfer)| {
                let remaining_bytes = transfer.remaining_wire_bytes();
                let serialization =
                    serialization_duration(remaining_bytes, contact.bitrate_bps);
                let finish_time = current_time.saturating_add(serialization);
                let can_finish_in_contact = remaining_bytes <= remaining_capacity
                    && finish_time <= contact.ends_at()
                    && finish_time <= transfer.fresh_until;

                Candidate {
                    index,
                    can_finish_in_contact,
                    fresh_until: transfer.fresh_until,
                    priority: transfer.priority,
                    remaining_bytes,
                }
            })
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            break;
        }

        candidates.sort_by(compare_candidates);
        let chosen_index = candidates[0].index;
        let transfer = &mut transfers[chosen_index];
        let wire_index = transfer.next_wire;
        let wire = transfer.wires[wire_index].clone();
        let wire_len = wire.len() as u64;

        remaining_capacity = remaining_capacity.saturating_sub(wire_len);
        fragments.push(ScheduledFragment {
            transfer_id: transfer.transfer_id,
            wire_index,
            wire,
        });
        transfer.next_wire += 1;

        if transfer.is_complete() {
            completed.push(transfer.transfer_id);
        }
    }

    let used_bytes = capacity.saturating_sub(remaining_capacity);
    let final_time = contact.starts_at.saturating_add(
        serialization_duration(used_bytes, contact.bitrate_bps),
    );
    for transfer in transfers.iter() {
        if !transfer.is_complete()
            && transfer.fresh_until <= final_time
            && !expired.contains(&transfer.transfer_id)
        {
            expired.push(transfer.transfer_id);
        }
    }
    expired.sort();

    Ok(ContactSchedule {
        capacity_bytes: capacity,
        used_bytes,
        fragments,
        completed_transfers: completed,
        expired_transfers: expired,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    index: usize,
    can_finish_in_contact: bool,
    fresh_until: Duration,
    priority: u32,
    remaining_bytes: u64,
}

fn compare_candidates(left: &Candidate, right: &Candidate) -> Ordering {
    right
        .can_finish_in_contact
        .cmp(&left.can_finish_in_contact)
        .then_with(|| left.fresh_until.cmp(&right.fresh_until))
        .then_with(|| right.priority.cmp(&left.priority))
        .then_with(|| left.remaining_bytes.cmp(&right.remaining_bytes))
        .then_with(|| left.index.cmp(&right.index))
}

fn serialization_duration(bytes: u64, bitrate_bps: u64) -> Duration {
    if bitrate_bps == 0 {
        return Duration::MAX;
    }
    let bits = u128::from(bytes).saturating_mul(8);
    let nanos = bits
        .saturating_mul(1_000_000_000)
        .div_ceil(u128::from(bitrate_bps));
    let secs = (nanos / 1_000_000_000).min(u128::from(u64::MAX)) as u64;
    Duration::new(secs, (nanos % 1_000_000_000) as u32)
}

#[derive(Debug)]
pub enum ScheduleError {
    Fragment(FragmentError),
    EmptyTransfer,
    ZeroCapacity,
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fragment(error) => write!(f, "fragment scheduling input: {error}"),
            Self::EmptyTransfer => write!(f, "scheduled transfer has no fragment wires"),
            Self::ZeroCapacity => write!(f, "contact has zero usable wire capacity"),
        }
    }
}

impl std::error::Error for ScheduleError {}

impl From<FragmentError> for ScheduleError {
    fn from(value: FragmentError) -> Self {
        Self::Fragment(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fragment_transport::FragmentAssembler;

    fn key() -> FragmentKey {
        FragmentKey::new([0x71; 32])
    }

    #[test]
    fn urgent_finishable_transfer_beats_later_high_priority_bulk() {
        let key = key();
        let urgent = b"fresh-answer".repeat(20);
        let bulk = vec![0xA5; 16 * 1024];

        let mut transfers = vec![
            PendingTransfer::from_bytes(
                &bulk,
                220,
                &key,
                Duration::from_secs(60),
                100,
            )
            .unwrap(),
            PendingTransfer::from_bytes(
                &urgent,
                220,
                &key,
                Duration::from_secs(5),
                1,
            )
            .unwrap(),
        ];

        let urgent_id = transfers[1].transfer_id;
        let schedule = schedule_contact(
            &mut transfers,
            ContactBudget {
                starts_at: Duration::ZERO,
                duration: Duration::from_secs(3),
                bitrate_bps: 2_000,
                max_wire_bytes: None,
            },
        )
        .unwrap();

        assert_eq!(schedule.fragments[0].transfer_id, urgent_id);
        assert!(schedule.completed_transfers.contains(&urgent_id));
    }

    #[test]
    fn expired_transfer_consumes_no_contact_bytes() {
        let key = key();
        let stale = b"stale".repeat(200);
        let fresh = b"fresh".repeat(200);

        let mut transfers = vec![
            PendingTransfer::from_bytes(
                &stale,
                220,
                &key,
                Duration::from_secs(4),
                100,
            )
            .unwrap(),
            PendingTransfer::from_bytes(
                &fresh,
                220,
                &key,
                Duration::from_secs(20),
                1,
            )
            .unwrap(),
        ];
        let stale_id = transfers[0].transfer_id;

        let schedule = schedule_contact(
            &mut transfers,
            ContactBudget {
                starts_at: Duration::from_secs(5),
                duration: Duration::from_secs(2),
                bitrate_bps: 10_000,
                max_wire_bytes: None,
            },
        )
        .unwrap();

        assert!(schedule.expired_transfers.contains(&stale_id));
        assert!(schedule
            .fragments
            .iter()
            .all(|fragment| fragment.transfer_id != stale_id));
    }

    #[test]
    fn partial_progress_resumes_across_contacts_and_reconstructs() {
        let key = key();
        let input = (0..12 * 1024)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();

        let transfer = PendingTransfer::from_bytes(
            &input,
            220,
            &key,
            Duration::from_secs(60),
            10,
        )
        .unwrap();
        let transfer_id = transfer.transfer_id;
        let mut transfers = vec![transfer];

        let first = schedule_contact(
            &mut transfers,
            ContactBudget {
                starts_at: Duration::ZERO,
                duration: Duration::from_secs(1),
                bitrate_bps: 4_000,
                max_wire_bytes: None,
            },
        )
        .unwrap();
        assert!(!first.fragments.is_empty());
        assert!(!first.completed_transfers.contains(&transfer_id));

        let second = schedule_contact(
            &mut transfers,
            ContactBudget {
                starts_at: Duration::from_secs(10),
                duration: Duration::from_secs(30),
                bitrate_bps: 20_000,
                max_wire_bytes: None,
            },
        )
        .unwrap();
        assert!(second.completed_transfers.contains(&transfer_id));

        let mut assembler = FragmentAssembler::new(32 * 1024);
        for fragment in first.fragments.iter().chain(second.fragments.iter()) {
            assembler.accept_wire(&fragment.wire, &key).unwrap();
        }

        assert!(assembler.is_complete());
        assert_eq!(assembler.reconstruct().unwrap(), input);
    }

    #[test]
    fn deadline_ages_as_contact_capacity_is_consumed() {
        let key = key();
        // First transfer is one short wire with the earliest deadline.
        // The second transfer could finish by 1.7s only if it started at t=0.
        // After the first wire consumes time, only one second-transfer wire
        // can still finish before freshness expiry.
        let first = b"first".repeat(10);
        let second = b"second".repeat(34);

        let mut transfers = vec![
            PendingTransfer::from_bytes(
                &first,
                220,
                &key,
                Duration::from_millis(800),
                10,
            )
            .unwrap(),
            PendingTransfer::from_bytes(
                &second,
                220,
                &key,
                Duration::from_millis(1_700),
                1,
            )
            .unwrap(),
        ];
        let second_id = transfers[1].transfer_id;

        let schedule = schedule_contact(
            &mut transfers,
            ContactBudget {
                starts_at: Duration::ZERO,
                duration: Duration::from_secs(10),
                bitrate_bps: 2_000,
                max_wire_bytes: None,
            },
        )
        .unwrap();

        // Once earlier scheduled bytes consume the freshness window, the
        // second transfer must not continue as if time were still t=0.
        let second_wires = schedule
            .fragments
            .iter()
            .filter(|fragment| fragment.transfer_id == second_id)
            .count();
        assert!(second_wires <= 1);
    }

    #[test]
    fn completion_feasibility_prevents_large_transfer_from_starving_small_one() {
        let key = key();
        let large = vec![0xCC; 64 * 1024];
        let small = b"small-remote-result".repeat(10);

        let mut transfers = vec![
            PendingTransfer::from_bytes(
                &large,
                220,
                &key,
                Duration::from_secs(10),
                1000,
            )
            .unwrap(),
            PendingTransfer::from_bytes(
                &small,
                220,
                &key,
                Duration::from_secs(12),
                1,
            )
            .unwrap(),
        ];
        let small_id = transfers[1].transfer_id;

        let schedule = schedule_contact(
            &mut transfers,
            ContactBudget {
                starts_at: Duration::ZERO,
                duration: Duration::from_secs(3),
                bitrate_bps: 2_000,
                max_wire_bytes: None,
            },
        )
        .unwrap();

        assert!(schedule.completed_transfers.contains(&small_id));
    }
}
