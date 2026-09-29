use std::env;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
struct Outage {
    period: Duration,
    down_for: Duration,
}

impl Outage {
    fn validate(self) -> Result<(), String> {
        if self.period.is_zero() || self.down_for >= self.period {
            return Err(
                "outage requires 0 < down_for < period".to_owned(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct ShaperConfig {
    bitrate_bps: u64,
    chunk_bytes: usize,
    outage: Option<Outage>,
}

impl ShaperConfig {
    fn validate(self) -> Result<(), String> {
        if self.bitrate_bps == 0 {
            return Err("bitrate_bps must be greater than zero".to_owned());
        }
        if self.chunk_bytes == 0 || self.chunk_bytes > 65_536 {
            return Err(
                "chunk_bytes must be in the range 1..=65536".to_owned(),
            );
        }
        if let Some(outage) = self.outage {
            outage.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct GateStats {
    scheduled_bytes: u64,
    serialization_time: Duration,
    outage_wait: Duration,
}

struct RealTimeGate {
    config: ShaperConfig,
    epoch: Instant,
    stats: GateStats,
}

impl RealTimeGate {
    fn new(config: ShaperConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self {
            config,
            epoch: Instant::now(),
            stats: GateStats::default(),
        })
    }

    fn schedule_bytes(&mut self, bytes: usize) -> Result<(), String> {
        let active = serialization_duration(bytes, self.config.bitrate_bps)?;
        self.stats.scheduled_bytes = self
            .stats
            .scheduled_bytes
            .saturating_add(bytes as u64);
        self.stats.serialization_time = self
            .stats
            .serialization_time
            .saturating_add(active);

        let mut remaining = active;

        while !remaining.is_zero() {
            let Some(outage) = self.config.outage else {
                thread::sleep(remaining);
                return Ok(());
            };

            let period_ns = outage.period.as_nanos();
            let down_ns = outage.down_for.as_nanos();
            let up_ns = period_ns
                .checked_sub(down_ns)
                .ok_or_else(|| "invalid outage arithmetic".to_owned())?;
            let phase = self.epoch.elapsed().as_nanos() % period_ns;

            if phase >= up_ns {
                let wait_ns = period_ns - phase;
                let wait = duration_from_nanos(wait_ns)?;
                thread::sleep(wait);
                self.stats.outage_wait =
                    self.stats.outage_wait.saturating_add(wait);
                continue;
            }

            let usable_ns = up_ns - phase;
            let remaining_ns = remaining.as_nanos();
            let step_ns = remaining_ns.min(usable_ns);
            let step = duration_from_nanos(step_ns)?;

            thread::sleep(step);
            remaining = remaining.saturating_sub(step);
        }

        Ok(())
    }

    fn stats(&self) -> GateStats {
        self.stats
    }
}

#[derive(Debug, Clone, Copy)]
struct RelayStats {
    bytes: u64,
    reads: u64,
    started: Instant,
    finished: Instant,
}

impl RelayStats {
    fn elapsed(self) -> Duration {
        self.finished.saturating_duration_since(self.started)
    }

    fn observed_bps(self) -> u64 {
        rate_bps(self.bytes, self.elapsed())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("g7-shaper-proxy: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 7 {
        return Err(usage());
    }

    let listen_addr = args[1]
        .parse::<SocketAddr>()
        .map_err(|_| "listen_addr must be a literal IP:port".to_owned())?;
    let upstream_addr = args[2]
        .parse::<SocketAddr>()
        .map_err(|_| "upstream_addr must be a literal IP:port".to_owned())?;
    let bitrate_bps = parse_u64(&args[3], "bitrate_bps")?;
    let chunk_bytes = parse_usize(&args[4], "chunk_bytes")?;
    let outage_period_ms = parse_u64(&args[5], "outage_period_ms")?;
    let outage_down_ms = parse_u64(&args[6], "outage_down_ms")?;

    let outage = if outage_period_ms == 0 && outage_down_ms == 0 {
        None
    } else {
        Some(Outage {
            period: Duration::from_millis(outage_period_ms),
            down_for: Duration::from_millis(outage_down_ms),
        })
    };

    let config = ShaperConfig {
        bitrate_bps,
        chunk_bytes,
        outage,
    };
    config.validate()?;

    let listener = TcpListener::bind(listen_addr)
        .map_err(|error| format!("bind {listen_addr}: {error}"))?;
    let actual = listener
        .local_addr()
        .map_err(|error| format!("read listen address: {error}"))?;

    println!(
        "G7_SHAPER_LISTEN addr={actual} upstream={upstream_addr} aggregate_bps={} chunk_bytes={} outage_period_ms={} outage_down_ms={}",
        config.bitrate_bps,
        config.chunk_bytes,
        outage_period_ms,
        outage_down_ms,
    );

    let (downstream, downstream_addr) = listener
        .accept()
        .map_err(|error| format!("accept downstream: {error}"))?;
    let upstream = TcpStream::connect(upstream_addr)
        .map_err(|error| format!("connect upstream {upstream_addr}: {error}"))?;

    downstream
        .set_nodelay(true)
        .map_err(|error| format!("downstream TCP_NODELAY: {error}"))?;
    upstream
        .set_nodelay(true)
        .map_err(|error| format!("upstream TCP_NODELAY: {error}"))?;

    println!(
        "G7_SHAPER_CONNECTED downstream={downstream_addr} upstream={upstream_addr}"
    );

    let gate = Arc::new(Mutex::new(RealTimeGate::new(config)?));
    let started = Instant::now();

    let down_read = downstream
        .try_clone()
        .map_err(|error| format!("clone downstream reader: {error}"))?;
    let down_write = downstream;
    let up_read = upstream
        .try_clone()
        .map_err(|error| format!("clone upstream reader: {error}"))?;
    let up_write = upstream;

    let gate_up = Arc::clone(&gate);
    let toward_upstream = thread::spawn(move || {
        relay_capped(
            "downstream_to_upstream",
            down_read,
            up_write,
            gate_up,
            config.chunk_bytes,
        )
    });

    let gate_down = Arc::clone(&gate);
    let toward_downstream = thread::spawn(move || {
        relay_capped(
            "upstream_to_downstream",
            up_read,
            down_write,
            gate_down,
            config.chunk_bytes,
        )
    });

    let up_stats = toward_upstream
        .join()
        .map_err(|_| "upstream relay thread panicked".to_owned())??;
    let down_stats = toward_downstream
        .join()
        .map_err(|_| "downstream relay thread panicked".to_owned())??;

    let elapsed = started.elapsed();
    let gate_stats = gate
        .lock()
        .map_err(|_| "shaper gate lock poisoned".to_owned())?
        .stats();
    let total_bytes = up_stats.bytes.saturating_add(down_stats.bytes);

    println!(
        "G7_SHAPER_DIRECTION name=downstream_to_upstream bytes={} reads={} elapsed_ms={} observed_bps={}",
        up_stats.bytes,
        up_stats.reads,
        up_stats.elapsed().as_millis(),
        up_stats.observed_bps(),
    );
    println!(
        "G7_SHAPER_DIRECTION name=upstream_to_downstream bytes={} reads={} elapsed_ms={} observed_bps={}",
        down_stats.bytes,
        down_stats.reads,
        down_stats.elapsed().as_millis(),
        down_stats.observed_bps(),
    );
    println!(
        "G7_SHAPER_PASS aggregate_target_bps={} total_bytes={} wall_elapsed_ms={} wall_observed_bps={} serialization_ms={} outage_wait_ms={}",
        config.bitrate_bps,
        total_bytes,
        elapsed.as_millis(),
        rate_bps(total_bytes, elapsed),
        gate_stats.serialization_time.as_millis(),
        gate_stats.outage_wait.as_millis(),
    );

    Ok(())
}

fn relay_capped(
    direction: &'static str,
    mut reader: TcpStream,
    mut writer: TcpStream,
    gate: Arc<Mutex<RealTimeGate>>,
    chunk_bytes: usize,
) -> Result<RelayStats, String> {
    let started = Instant::now();
    let mut buffer = vec![0_u8; chunk_bytes];
    let mut bytes = 0_u64;
    let mut reads = 0_u64;

    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("{direction} read: {error}"))?;

        if count == 0 {
            let _ = writer.shutdown(Shutdown::Write);
            break;
        }

        {
            let mut gate = gate
                .lock()
                .map_err(|_| "shaper gate lock poisoned".to_owned())?;
            gate.schedule_bytes(count)?;
        }

        writer
            .write_all(&buffer[..count])
            .map_err(|error| format!("{direction} write: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("{direction} flush: {error}"))?;

        bytes = bytes.saturating_add(count as u64);
        reads = reads.saturating_add(1);
    }

    Ok(RelayStats {
        bytes,
        reads,
        started,
        finished: Instant::now(),
    })
}

fn serialization_duration(
    bytes: usize,
    bitrate_bps: u64,
) -> Result<Duration, String> {
    if bitrate_bps == 0 {
        return Err("bitrate_bps must be greater than zero".to_owned());
    }

    let bits = (bytes as u128)
        .checked_mul(8)
        .ok_or_else(|| "byte count overflow".to_owned())?;
    let nanos = bits
        .checked_mul(1_000_000_000)
        .ok_or_else(|| "serialization duration overflow".to_owned())?
        .div_ceil(u128::from(bitrate_bps));

    duration_from_nanos(nanos)
}

fn duration_from_nanos(nanos: u128) -> Result<Duration, String> {
    let seconds = nanos / 1_000_000_000;
    let subsec = (nanos % 1_000_000_000) as u32;
    let seconds = u64::try_from(seconds)
        .map_err(|_| "duration exceeds u64 seconds".to_owned())?;

    Ok(Duration::new(seconds, subsec))
}

fn rate_bps(bytes: u64, elapsed: Duration) -> u64 {
    let nanos = elapsed.as_nanos();
    if nanos == 0 {
        return 0;
    }

    let bits = u128::from(bytes).saturating_mul(8);
    (bits
        .saturating_mul(1_000_000_000)
        .checked_div(nanos)
        .unwrap_or(0))
        .min(u128::from(u64::MAX)) as u64
}

fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an unsigned integer"))
}

fn parse_usize(value: &str, name: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))
}

fn usage() -> String {
    [
        "usage:",
        "  g7-shaper-proxy <listen_ip:port> <upstream_ip:port> <aggregate_bps> <chunk_bytes> <outage_period_ms> <outage_down_ms>",
        "",
        "examples:",
        "  g7-shaper-proxy 0.0.0.0:45200 192.168.1.30:45123 1000 16 0 0",
        "  g7-shaper-proxy 0.0.0.0:45200 192.168.1.30:45123 30 1 15000 4000",
        "",
        "The cap is shared across both directions and preserves byte fidelity.",
        "Use 0 0 for no periodic outage.",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_duration_matches_exact_rate_math() {
        assert_eq!(
            serialization_duration(125, 1_000).unwrap(),
            Duration::from_secs(1)
        );
        assert_eq!(
            serialization_duration(1, 10).unwrap(),
            Duration::from_millis(800)
        );
    }

    #[test]
    fn rate_math_is_stable() {
        assert_eq!(rate_bps(125, Duration::from_secs(1)), 1_000);
        assert_eq!(rate_bps(0, Duration::from_secs(1)), 0);
    }

    #[test]
    fn validates_outage_and_config() {
        assert!(Outage {
            period: Duration::from_secs(10),
            down_for: Duration::from_secs(2),
        }
        .validate()
        .is_ok());

        assert!(Outage {
            period: Duration::from_secs(10),
            down_for: Duration::from_secs(10),
        }
        .validate()
        .is_err());

        assert!(ShaperConfig {
            bitrate_bps: 0,
            chunk_bytes: 1,
            outage: None,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn tiny_high_rate_gate_does_not_corrupt_accounting() {
        let mut gate = RealTimeGate::new(ShaperConfig {
            bitrate_bps: 8_000_000,
            chunk_bytes: 8,
            outage: None,
        })
        .unwrap();

        gate.schedule_bytes(8).unwrap();
        let stats = gate.stats();

        assert_eq!(stats.scheduled_bytes, 8);
        assert_eq!(
            stats.serialization_time,
            Duration::from_micros(8)
        );
    }
}
