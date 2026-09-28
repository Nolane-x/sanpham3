use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptMeasurement {
    pub elapsed: Duration,
    pub useful_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptSample {
    pub success: bool,
    pub elapsed: Duration,
    pub useful_bytes: usize,
    pub detail: Option<String>,
}

impl AttemptSample {
    pub fn success(measurement: AttemptMeasurement) -> Self {
        Self {
            success: true,
            elapsed: measurement.elapsed,
            useful_bytes: measurement.useful_bytes,
            detail: None,
        }
    }

    pub fn failure(elapsed: Duration, detail: impl Into<String>) -> Self {
        Self {
            success: false,
            elapsed,
            useful_bytes: 0,
            detail: Some(detail.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSeriesSummary {
    pub attempts: u32,
    pub successes: u32,
    pub failures: u32,
    pub loss_ppm: u32,
    pub min_rtt: Option<Duration>,
    pub median_rtt: Option<Duration>,
    pub p95_rtt: Option<Duration>,
    pub total_useful_bytes: u64,
    pub observed_useful_bitrate_bps: u64,
    pub longest_failure_run: u32,
    pub state_transitions: u32,
    pub intermittent: bool,
}

#[derive(Debug, Clone)]
pub struct TcpConnectResult {
    pub destination: SocketAddr,
    pub source: SocketAddr,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct TinyHttpsResult {
    pub destination: SocketAddr,
    pub source: SocketAddr,
    pub tcp_connect_elapsed: Duration,
    pub total_elapsed: Duration,
    pub status_code: u16,
    pub response_bytes: usize,
}

pub fn summarize(samples: &[AttemptSample]) -> ProbeSeriesSummary {
    if samples.is_empty() {
        return ProbeSeriesSummary {
            attempts: 0,
            successes: 0,
            failures: 0,
            loss_ppm: 0,
            min_rtt: None,
            median_rtt: None,
            p95_rtt: None,
            total_useful_bytes: 0,
            observed_useful_bitrate_bps: 0,
            longest_failure_run: 0,
            state_transitions: 0,
            intermittent: false,
        };
    }

    let attempts = samples.len() as u32;
    let successes = samples.iter().filter(|sample| sample.success).count() as u32;
    let failures = attempts - successes;
    let loss_ppm = ((u64::from(failures) * 1_000_000) / u64::from(attempts))
        .min(1_000_000) as u32;

    let mut successful_rtts = samples
        .iter()
        .filter(|sample| sample.success)
        .map(|sample| sample.elapsed)
        .collect::<Vec<_>>();
    successful_rtts.sort_unstable();

    let min_rtt = successful_rtts.first().copied();
    let median_rtt = percentile(&successful_rtts, 50);
    let p95_rtt = percentile(&successful_rtts, 95);

    let total_useful_bytes = samples
        .iter()
        .map(|sample| sample.useful_bytes as u64)
        .sum::<u64>();
    let total_elapsed = samples
        .iter()
        .fold(Duration::ZERO, |total, sample| total + sample.elapsed);

    let observed_useful_bitrate_bps = if total_elapsed.is_zero() {
        0
    } else {
        let bits = u128::from(total_useful_bytes) * 8;
        let nanos = total_elapsed.as_nanos();
        if nanos == 0 {
            0
        } else {
            ((bits * 1_000_000_000) / nanos)
                .min(u128::from(u64::MAX)) as u64
        }
    };

    let mut longest_failure_run = 0_u32;
    let mut current_failure_run = 0_u32;
    let mut state_transitions = 0_u32;
    let mut previous = None;

    for sample in samples {
        if sample.success {
            current_failure_run = 0;
        } else {
            current_failure_run = current_failure_run.saturating_add(1);
            longest_failure_run = longest_failure_run.max(current_failure_run);
        }

        if let Some(previous_success) = previous {
            if previous_success != sample.success {
                state_transitions = state_transitions.saturating_add(1);
            }
        }
        previous = Some(sample.success);
    }

    ProbeSeriesSummary {
        attempts,
        successes,
        failures,
        loss_ppm,
        min_rtt,
        median_rtt,
        p95_rtt,
        total_useful_bytes,
        observed_useful_bitrate_bps,
        longest_failure_run,
        state_transitions,
        intermittent: successes > 0 && failures > 0,
    }
}

pub fn run_series<F>(
    attempts: usize,
    pause: Duration,
    mut attempt: F,
) -> Vec<AttemptSample>
where
    F: FnMut() -> io::Result<AttemptMeasurement>,
{
    let mut samples = Vec::with_capacity(attempts);

    for index in 0..attempts {
        let started = Instant::now();
        let sample = match attempt() {
            Ok(measurement) => AttemptSample::success(measurement),
            Err(error) => AttemptSample::failure(
                started.elapsed(),
                error.to_string(),
            ),
        };
        samples.push(sample);

        if index + 1 < attempts && !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }

    samples
}

#[cfg(target_os = "linux")]
pub fn tcp_connect_device(
    interface: &str,
    destination: SocketAddr,
    timeout: Duration,
) -> io::Result<TcpConnectResult> {
    let started = Instant::now();
    let stream = connect_bound_device(interface, destination, timeout)?;
    let source = stream.local_addr()?;

    Ok(TcpConnectResult {
        destination,
        source,
        elapsed: started.elapsed(),
    })
}

#[cfg(not(target_os = "linux"))]
pub fn tcp_connect_device(
    _interface: &str,
    _destination: SocketAddr,
    _timeout: Duration,
) -> io::Result<TcpConnectResult> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "interface-bound TCP probing is Linux-only",
    ))
}

pub fn tcp_connect(
    source_ip: Option<IpAddr>,
    destination: SocketAddr,
    timeout: Duration,
) -> io::Result<TcpConnectResult> {
    let started = Instant::now();
    let stream = connect_bound(source_ip, destination, timeout)?;
    let source = stream.local_addr()?;

    Ok(TcpConnectResult {
        destination,
        source,
        elapsed: started.elapsed(),
    })
}

pub fn tiny_https_head(
    source_ip: Option<IpAddr>,
    destination: SocketAddr,
    server_name: &str,
    path: &str,
    timeout: Duration,
    max_response_bytes: usize,
) -> io::Result<TinyHttpsResult> {
    if max_response_bytes < 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "max_response_bytes must be at least 16",
        ));
    }
    if !path.starts_with('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTPS path must start with '/'",
        ));
    }

    let started = Instant::now();
    let tcp_started = Instant::now();
    let stream = connect_bound(source_ip, destination, timeout)?;
    let tcp_connect_elapsed = tcp_started.elapsed();
    let source = stream.local_addr()?;

    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let roots = RootCertStore::from_iter(
        webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
    );
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name_text = server_name.to_owned();
    let server_name = ServerName::try_from(server_name_text.clone()).map_err(
        |_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid TLS server name",
            )
        },
    )?;
    let connection = ClientConnection::new(Arc::new(config), server_name)
        .map_err(io::Error::other)?;
    let mut tls = StreamOwned::new(connection, stream);

    let request = format!(
        "HEAD {path} HTTP/1.1\r\nHost: {server_name}\r\nUser-Agent: sanpham3-path-probe/0\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        server_name = server_name_text,
    );
    tls.write_all(request.as_bytes())?;
    tls.flush()?;

    let mut received = Vec::with_capacity(max_response_bytes.min(4096));
    let mut chunk = [0_u8; 512];

    while received.len() < max_response_bytes {
        let remaining = max_response_bytes - received.len();
        let read_len = remaining.min(chunk.len());

        match tls.read(&mut chunk[..read_len]) {
            Ok(0) => break,
            Ok(count) => {
                received.extend_from_slice(&chunk[..count]);
                if received.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }

    let status_code = parse_http_status(&received)?;
    let total_elapsed = started.elapsed();

    Ok(TinyHttpsResult {
        destination,
        source,
        tcp_connect_elapsed,
        total_elapsed,
        status_code,
        response_bytes: received.len(),
    })
}

#[cfg(target_os = "linux")]
fn connect_bound_device(
    interface: &str,
    destination: SocketAddr,
    timeout: Duration,
) -> io::Result<TcpStream> {
    if interface.is_empty() || interface.as_bytes().contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid interface name",
        ));
    }

    let domain = Domain::for_address(destination);
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.bind_device(Some(interface.as_bytes()))?;
    socket.connect_timeout(&SockAddr::from(destination), timeout)?;
    socket.set_nodelay(true)?;

    Ok(socket.into())
}

fn connect_bound(
    source_ip: Option<IpAddr>,
    destination: SocketAddr,
    timeout: Duration,
) -> io::Result<TcpStream> {
    let domain = Domain::for_address(destination);
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

    if let Some(source_ip) = source_ip {
        if source_ip.is_ipv4() != destination.is_ipv4() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source and destination address families differ",
            ));
        }

        socket.bind(&SockAddr::from(SocketAddr::new(source_ip, 0)))?;
    }

    socket.connect_timeout(&SockAddr::from(destination), timeout)?;
    socket.set_nodelay(true)?;

    Ok(socket.into())
}

fn parse_http_status(bytes: &[u8]) -> io::Result<u16> {
    let line_end = bytes
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP response has no status line",
            )
        })?;

    let status_line = std::str::from_utf8(&bytes[..line_end]).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP status line is not UTF-8",
        )
    })?;

    let mut parts = status_line.split_whitespace();
    let version = parts.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing HTTP version")
    })?;
    if !version.starts_with("HTTP/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP version",
        ));
    }

    let code = parts
        .next()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing HTTP status")
        })?
        .parse::<u16>()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid HTTP status code",
            )
        })?;

    if !(100..=599).contains(&code) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP status outside valid range",
        ));
    }

    Ok(code)
}

fn percentile(values: &[Duration], percentile: usize) -> Option<Duration> {
    if values.is_empty() {
        return None;
    }

    let rank = ((values.len() - 1) * percentile + 99) / 100;
    values.get(rank.min(values.len() - 1)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener};
    use std::thread;

    #[test]
    fn series_summary_captures_loss_and_intermittency() {
        let samples = vec![
            AttemptSample::success(AttemptMeasurement {
                elapsed: Duration::from_millis(10),
                useful_bytes: 100,
            }),
            AttemptSample::failure(
                Duration::from_millis(20),
                "timeout",
            ),
            AttemptSample::success(AttemptMeasurement {
                elapsed: Duration::from_millis(30),
                useful_bytes: 50,
            }),
            AttemptSample::failure(
                Duration::from_millis(40),
                "timeout",
            ),
            AttemptSample::failure(
                Duration::from_millis(50),
                "timeout",
            ),
        ];

        let summary = summarize(&samples);
        assert_eq!(summary.attempts, 5);
        assert_eq!(summary.successes, 2);
        assert_eq!(summary.failures, 3);
        assert_eq!(summary.loss_ppm, 600_000);
        assert_eq!(summary.min_rtt, Some(Duration::from_millis(10)));
        assert_eq!(summary.median_rtt, Some(Duration::from_millis(10)));
        assert_eq!(summary.p95_rtt, Some(Duration::from_millis(30)));
        assert_eq!(summary.total_useful_bytes, 150);
        assert_eq!(summary.longest_failure_run, 2);
        assert_eq!(summary.state_transitions, 3);
        assert!(summary.intermittent);
    }

    #[test]
    fn tcp_connect_can_bind_explicit_source_address() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let destination = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (_stream, peer) = listener.accept().unwrap();
            peer
        });

        let result = tcp_connect(
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            destination,
            Duration::from_secs(2),
        )
        .unwrap();

        assert_eq!(result.source.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(server.join().unwrap().ip(), result.source.ip());
    }

    #[test]
    fn repeated_tcp_series_reports_zero_loss() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let destination = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            for _ in 0..3 {
                let _ = listener.accept().unwrap();
            }
        });

        let samples = run_series(3, Duration::ZERO, || {
            let result = tcp_connect(
                Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                destination,
                Duration::from_secs(1),
            )?;

            Ok(AttemptMeasurement {
                elapsed: result.elapsed,
                useful_bytes: 1,
            })
        });

        let summary = summarize(&samples);
        assert_eq!(summary.attempts, 3);
        assert_eq!(summary.successes, 3);
        assert_eq!(summary.loss_ppm, 0);
        assert!(!summary.intermittent);

        server.join().unwrap();
    }

    #[test]
    fn parses_http_status_line() {
        assert_eq!(
            parse_http_status(b"HTTP/1.1 204 No Content\r\nX: y\r\n\r\n")
                .unwrap(),
            204
        );
        assert!(parse_http_status(b"garbage\r\n").is_err());
    }
}
