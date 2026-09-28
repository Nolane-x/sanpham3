use connectivity_core::{Bundle, BundlePriority, DtnQueue};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAGIC: [u8; 4] = *b"SP3D";
const VERSION: u8 = 0;
const HEADER_LEN: usize = 9;
const ENTRY_FIXED_LEN: usize = 25;
const MAX_ENTRIES: usize = 100_000;
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_SPOOL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum SpoolError {
    Io(io::Error),
    WrongMagic,
    UnsupportedVersion(u8),
    Truncated,
    TooManyEntries,
    PayloadTooLarge,
    FileTooLarge,
    InvalidPriority(u8),
    ClockBeforeEpoch,
    TimeOverflow,
    DuplicateBundle(u64),
    TrailingBytes,
}

impl From<io::Error> for SpoolError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn save_queue(
    path: impl AsRef<Path>,
    queue: &DtnQueue,
    monotonic_now: Instant,
    wall_now: SystemTime,
) -> Result<usize, SpoolError> {
    let path = path.as_ref();
    let wall_ms = unix_millis(wall_now)?;

    let live = queue
        .iter()
        .filter_map(|bundle| {
            let elapsed =
                monotonic_now.saturating_duration_since(bundle.created_at);
            let remaining = bundle.ttl.saturating_sub(elapsed);
            (!remaining.is_zero()).then_some((bundle, remaining))
        })
        .collect::<Vec<_>>();

    if live.len() > MAX_ENTRIES {
        return Err(SpoolError::TooManyEntries);
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.push(VERSION);
    bytes.extend_from_slice(&(live.len() as u32).to_be_bytes());

    for (bundle, remaining) in &live {
        if bundle.payload.len() > MAX_PAYLOAD_BYTES {
            return Err(SpoolError::PayloadTooLarge);
        }

        let remaining_ms = u64::try_from(remaining.as_millis())
            .map_err(|_| SpoolError::TimeOverflow)?;
        let expires_ms = wall_ms
            .checked_add(remaining_ms)
            .ok_or(SpoolError::TimeOverflow)?;

        bytes.extend_from_slice(&bundle.id.to_be_bytes());
        bytes.push(encode_priority(bundle.priority));
        bytes.extend_from_slice(&expires_ms.to_be_bytes());
        bytes.extend_from_slice(&bundle.attempts.to_be_bytes());
        bytes.extend_from_slice(
            &(bundle.payload.len() as u32).to_be_bytes(),
        );
        bytes.extend_from_slice(&bundle.payload);
    }

    if bytes.len() as u64 > MAX_SPOOL_BYTES {
        return Err(SpoolError::FileTooLarge);
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let tmp_path = path.with_extension("sp3d.tmp");
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }

    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp_path, path)?;

    Ok(live.len())
}

pub fn load_queue(
    path: impl AsRef<Path>,
    monotonic_now: Instant,
    wall_now: SystemTime,
) -> Result<DtnQueue, SpoolError> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_SPOOL_BYTES {
        return Err(SpoolError::FileTooLarge);
    }

    let bytes = fs::read(path)?;
    decode_queue(&bytes, monotonic_now, wall_now)
}

pub fn decode_queue(
    bytes: &[u8],
    monotonic_now: Instant,
    wall_now: SystemTime,
) -> Result<DtnQueue, SpoolError> {
    if bytes.len() < HEADER_LEN {
        return Err(SpoolError::Truncated);
    }
    if bytes[0..4] != MAGIC {
        return Err(SpoolError::WrongMagic);
    }
    if bytes[4] != VERSION {
        return Err(SpoolError::UnsupportedVersion(bytes[4]));
    }

    let count = u32::from_be_bytes(
        bytes[5..9]
            .try_into()
            .map_err(|_| SpoolError::Truncated)?,
    ) as usize;
    if count > MAX_ENTRIES {
        return Err(SpoolError::TooManyEntries);
    }

    let wall_ms = unix_millis(wall_now)?;
    let mut cursor = HEADER_LEN;
    let mut queue = DtnQueue::new();

    for _ in 0..count {
        if bytes.len().saturating_sub(cursor) < ENTRY_FIXED_LEN {
            return Err(SpoolError::Truncated);
        }

        let id = u64::from_be_bytes(
            bytes[cursor..cursor + 8]
                .try_into()
                .map_err(|_| SpoolError::Truncated)?,
        );
        cursor += 8;

        let priority = decode_priority(bytes[cursor])?;
        cursor += 1;

        let expires_ms = u64::from_be_bytes(
            bytes[cursor..cursor + 8]
                .try_into()
                .map_err(|_| SpoolError::Truncated)?,
        );
        cursor += 8;

        let attempts = u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| SpoolError::Truncated)?,
        );
        cursor += 4;

        let payload_len = u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .map_err(|_| SpoolError::Truncated)?,
        ) as usize;
        cursor += 4;

        if payload_len > MAX_PAYLOAD_BYTES {
            return Err(SpoolError::PayloadTooLarge);
        }

        let payload = bytes
            .get(cursor..cursor + payload_len)
            .ok_or(SpoolError::Truncated)?
            .to_vec();
        cursor += payload_len;

        if expires_ms <= wall_ms {
            continue;
        }

        let remaining = Duration::from_millis(expires_ms - wall_ms);
        let bundle = Bundle {
            id,
            priority,
            created_at: monotonic_now,
            ttl: remaining,
            payload,
            attempts,
        };

        if !queue.push_unique(bundle) {
            return Err(SpoolError::DuplicateBundle(id));
        }
    }

    if cursor != bytes.len() {
        return Err(SpoolError::TrailingBytes);
    }

    Ok(queue)
}

fn unix_millis(time: SystemTime) -> Result<u64, SpoolError> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SpoolError::ClockBeforeEpoch)?;

    u64::try_from(duration.as_millis())
        .map_err(|_| SpoolError::TimeOverflow)
}

fn encode_priority(priority: BundlePriority) -> u8 {
    match priority {
        BundlePriority::Bulk => 0,
        BundlePriority::Normal => 1,
        BundlePriority::Urgent => 2,
    }
}

fn decode_priority(value: u8) -> Result<BundlePriority, SpoolError> {
    match value {
        0 => Ok(BundlePriority::Bulk),
        1 => Ok(BundlePriority::Normal),
        2 => Ok(BundlePriority::Urgent),
        other => Err(SpoolError::InvalidPriority(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_queue(now: Instant) -> DtnQueue {
        let mut queue = DtnQueue::new();
        assert!(queue.push_unique(Bundle {
            id: 10,
            priority: BundlePriority::Urgent,
            created_at: now,
            ttl: Duration::from_secs(60),
            payload: vec![1, 2, 3, 4],
            attempts: 7,
        }));
        assert!(queue.push_unique(Bundle {
            id: 20,
            priority: BundlePriority::Bulk,
            created_at: now,
            ttl: Duration::from_secs(1),
            payload: vec![9, 8],
            attempts: 1,
        }));
        queue
    }

    fn temp_path() -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "sanpham3-dtn-spool-{}-{suffix}.bin",
            std::process::id()
        ))
    }

    #[test]
    fn queue_survives_disk_roundtrip_with_remaining_ttl() {
        let mono = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let queue = sample_queue(mono);
        let path = temp_path();

        assert_eq!(save_queue(&path, &queue, mono, wall).unwrap(), 2);

        let restored = load_queue(
            &path,
            Instant::now(),
            wall + Duration::from_millis(500),
        )
        .unwrap();

        assert_eq!(restored.len(), 2);
        assert!(restored.contains(10));
        assert!(restored.contains(20));

        let urgent = restored
            .iter()
            .find(|bundle| bundle.id == 10)
            .unwrap();
        assert_eq!(urgent.priority, BundlePriority::Urgent);
        assert_eq!(urgent.payload, vec![1, 2, 3, 4]);
        assert_eq!(urgent.attempts, 7);
        assert!(urgent.ttl <= Duration::from_secs(60));
        assert!(urgent.ttl >= Duration::from_secs(59));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn expired_bundle_is_not_restored_after_downtime() {
        let mono = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let queue = sample_queue(mono);
        let path = temp_path();

        save_queue(&path, &queue, mono, wall).unwrap();

        let restored = load_queue(
            &path,
            Instant::now(),
            wall + Duration::from_secs(2),
        )
        .unwrap();

        assert_eq!(restored.len(), 1);
        assert!(restored.contains(10));
        assert!(!restored.contains(20));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_spool_is_rejected() {
        let now = Instant::now();
        let wall = UNIX_EPOCH + Duration::from_secs(2_000_000_000);

        assert!(matches!(
            decode_queue(b"bad", now, wall),
            Err(SpoolError::Truncated)
        ));

        let mut wrong_magic = vec![0_u8; HEADER_LEN];
        wrong_magic[0..4].copy_from_slice(b"NOPE");
        assert!(matches!(
            decode_queue(&wrong_magic, now, wall),
            Err(SpoolError::WrongMagic)
        ));
    }
}
