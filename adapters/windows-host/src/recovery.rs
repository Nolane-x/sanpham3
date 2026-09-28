use crate::{inventory_adapter_paths, WindowsAdapterPath};
use connectivity_core::{
    LinkObservation, MeasuredPathEvidence, NodeId, ProbeKind, ProbeStatus,
    Reachability, RecoveryLedger, Transport,
};
use path_probe::{
    run_series, summarize, tcp_connect, tiny_https_head, AttemptMeasurement,
    HttpsProbeTarget, ProbeSeriesSummary,
};
use std::io;
use std::net::{IpAddr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct WindowsDnsObservation {
    pub interface: String,
    pub source: Option<IpAddr>,
    pub resolver: IpAddr,
    pub status: ProbeStatus,
    pub detail: String,
    pub elapsed: Option<Duration>,
    pub response_bytes: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct WindowsTcpObservation {
    pub interface: String,
    pub source: IpAddr,
    pub target: SocketAddr,
    pub status: ProbeStatus,
    pub detail: String,
    pub summary: ProbeSeriesSummary,
}

#[derive(Debug, Clone)]
pub struct WindowsHttpsObservation {
    pub interface: String,
    pub transport: Transport,
    pub source: Option<IpAddr>,
    pub target: HttpsProbeTarget,
    pub status: ProbeStatus,
    pub detail: String,
    pub summary: ProbeSeriesSummary,
}

#[derive(Debug)]
pub struct WindowsRecoverySnapshot {
    pub ledger: RecoveryLedger,
    pub adapters: Vec<WindowsAdapterPath>,
    pub dns: Vec<WindowsDnsObservation>,
    pub tcp: Vec<WindowsTcpObservation>,
    pub https: Vec<WindowsHttpsObservation>,
}

impl WindowsHttpsObservation {
    pub fn to_link_observation(
        &self,
        from: NodeId,
        to: NodeId,
        last_success_age: Duration,
    ) -> LinkObservation {
        series_to_link(
            &self.summary,
            self.status,
            from,
            to,
            self.transport,
            last_success_age,
        )
    }
}

impl WindowsRecoverySnapshot {
    pub fn internet_link_observations(
        &self,
        from: NodeId,
        to: NodeId,
        last_success_age: Duration,
    ) -> Vec<LinkObservation> {
        self.https
            .iter()
            .map(|observation| {
                observation.to_link_observation(
                    from,
                    to,
                    last_success_age,
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct WindowsRecoveryProbe {
    dns_timeout: Duration,
    tcp_timeout: Duration,
    tcp_attempts: usize,
    tcp_pause: Duration,
    https_targets: Vec<HttpsProbeTarget>,
    https_timeout: Duration,
    https_attempts: usize,
    https_pause: Duration,
}

impl Default for WindowsRecoveryProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsRecoveryProbe {
    pub fn new() -> Self {
        Self {
            dns_timeout: Duration::from_millis(900),
            tcp_timeout: Duration::from_millis(900),
            tcp_attempts: 3,
            tcp_pause: Duration::from_millis(80),
            https_targets: Vec::new(),
            https_timeout: Duration::from_millis(1500),
            https_attempts: 2,
            https_pause: Duration::from_millis(120),
        }
    }

    pub fn dns_timeout(mut self, timeout: Duration) -> Self {
        self.dns_timeout = timeout;
        self
    }

    pub fn tcp_policy(
        mut self,
        attempts: usize,
        timeout: Duration,
        pause: Duration,
    ) -> Self {
        self.tcp_attempts = attempts.max(1);
        self.tcp_timeout = timeout;
        self.tcp_pause = pause;
        self
    }

    pub fn https_targets(
        mut self,
        targets: impl IntoIterator<Item = HttpsProbeTarget>,
    ) -> Self {
        self.https_targets = targets.into_iter().collect();
        self
    }

    pub fn https_policy(
        mut self,
        attempts: usize,
        timeout: Duration,
        pause: Duration,
    ) -> Self {
        self.https_attempts = attempts.max(1);
        self.https_timeout = timeout;
        self.https_pause = pause;
        self
    }

    pub fn run(&self) -> Result<WindowsRecoverySnapshot, String> {
        let adapters = inventory_adapter_paths()?;
        let mut ledger = RecoveryLedger::new();
        let mut dns = Vec::new();
        let mut tcp = Vec::new();
        let mut https = Vec::new();

        for adapter in adapters.iter().filter(|adapter| adapter.available) {
            record_address_family(
                &mut ledger,
                adapter,
                ProbeKind::Ipv4,
                adapter.unicast.iter().any(IpAddr::is_ipv4),
            );
            record_address_family(
                &mut ledger,
                adapter,
                ProbeKind::Ipv6,
                adapter.unicast.iter().any(IpAddr::is_ipv6),
            );

            if self.https_targets.is_empty() {
                let id = format!("{}:https", adapter.name);
                ledger.register(&id, ProbeKind::TinyHttps);
                ledger.set_status(
                    &id,
                    ProbeStatus::Unsupported,
                    Some("no HTTPS probe target configured".to_owned()),
                );
            } else {
                for target in &self.https_targets {
                    let id = format!(
                        "{}:https:{}:{}",
                        adapter.name,
                        target.server_name,
                        target.address,
                    );
                    ledger.register(&id, ProbeKind::TinyHttps);

                    let source = select_source(adapter, target.address.ip());
                    let Some(source) = source else {
                        let detail =
                            "no usable local source address matches HTTPS target family"
                                .to_owned();
                        ledger.set_status(
                            &id,
                            ProbeStatus::Unsupported,
                            Some(detail.clone()),
                        );
                        https.push(WindowsHttpsObservation {
                            interface: adapter.name.clone(),
                            transport: adapter.transport,
                            source: None,
                            target: target.clone(),
                            status: ProbeStatus::Unsupported,
                            detail,
                            summary: summarize(&[]),
                        });
                        continue;
                    };

                    let samples = run_series(
                        self.https_attempts,
                        self.https_pause,
                        || {
                            let result = tiny_https_head(
                                Some(source),
                                target.address,
                                &target.server_name,
                                &target.path,
                                self.https_timeout,
                                target.max_response_bytes,
                            )?;

                            Ok(AttemptMeasurement {
                                elapsed: result.total_elapsed,
                                useful_bytes: result.response_bytes,
                            })
                        },
                    );
                    let summary = summarize(&samples);
                    let status = if summary.successes > 0 {
                        ProbeStatus::Succeeded
                    } else {
                        ProbeStatus::Failed
                    };
                    let detail = format_series_detail(&summary);
                    ledger.set_status(
                        &id,
                        status,
                        Some(detail.clone()),
                    );
                    https.push(WindowsHttpsObservation {
                        interface: adapter.name.clone(),
                        transport: adapter.transport,
                        source: Some(source),
                        target: target.clone(),
                        status,
                        detail,
                        summary,
                    });
                }
            }

            if adapter.dns_servers.is_empty() {
                let id = format!("{}:dns", adapter.name);
                ledger.register(&id, ProbeKind::Dns);
                ledger.set_status(
                    &id,
                    ProbeStatus::Unsupported,
                    Some("adapter exposes no DNS server".to_owned()),
                );
                continue;
            }

            for resolver in &adapter.dns_servers {
                let id = format!("{}:dns:{resolver}", adapter.name);
                ledger.register(&id, ProbeKind::Dns);

                if matches!(
                    resolver,
                    IpAddr::V6(address) if address.is_unicast_link_local()
                ) {
                    let detail =
                        "IPv6 link-local DNS needs interface scope; scoped probing is not implemented yet"
                            .to_owned();
                    ledger.set_status(
                        &id,
                        ProbeStatus::Unsupported,
                        Some(detail.clone()),
                    );
                    dns.push(WindowsDnsObservation {
                        interface: adapter.name.clone(),
                        source: None,
                        resolver: *resolver,
                        status: ProbeStatus::Unsupported,
                        detail,
                        elapsed: None,
                        response_bytes: None,
                    });
                    continue;
                }

                let source = select_source(adapter, *resolver);
                let Some(source) = source else {
                    let detail =
                        "no usable local source address matches resolver family"
                            .to_owned();
                    ledger.set_status(
                        &id,
                        ProbeStatus::Unsupported,
                        Some(detail.clone()),
                    );
                    dns.push(WindowsDnsObservation {
                        interface: adapter.name.clone(),
                        source: None,
                        resolver: *resolver,
                        status: ProbeStatus::Unsupported,
                        detail,
                        elapsed: None,
                        response_bytes: None,
                    });
                    continue;
                };

                match probe_dns_udp(source, *resolver, self.dns_timeout) {
                    Ok(success) => {
                        let detail = format!(
                            "source={} resolver={} elapsed_ms={} bytes={} rcode={}",
                            source,
                            resolver,
                            success.elapsed.as_millis(),
                            success.response_bytes,
                            success.rcode,
                        );
                        ledger.set_status(
                            &id,
                            ProbeStatus::Succeeded,
                            Some(detail.clone()),
                        );
                        dns.push(WindowsDnsObservation {
                            interface: adapter.name.clone(),
                            source: Some(source),
                            resolver: *resolver,
                            status: ProbeStatus::Succeeded,
                            detail,
                            elapsed: Some(success.elapsed),
                            response_bytes: Some(success.response_bytes),
                        });
                    }
                    Err(error) => {
                        let status = classify_probe_error(&error);
                        let detail = error.to_string();
                        ledger.set_status(
                            &id,
                            status,
                            Some(detail.clone()),
                        );
                        dns.push(WindowsDnsObservation {
                            interface: adapter.name.clone(),
                            source: Some(source),
                            resolver: *resolver,
                            status,
                            detail,
                            elapsed: None,
                            response_bytes: None,
                        });
                    }
                }

                let target = SocketAddr::new(*resolver, 53);
                let tcp_id = format!("{}:tcp:{target}", adapter.name);
                ledger.register(&tcp_id, ProbeKind::Tcp);

                let samples = run_series(
                    self.tcp_attempts,
                    self.tcp_pause,
                    || {
                        let result = tcp_connect(
                            Some(source),
                            target,
                            self.tcp_timeout,
                        )?;
                        Ok(AttemptMeasurement {
                            elapsed: result.elapsed,
                            useful_bytes: 0,
                        })
                    },
                );
                let summary = summarize(&samples);
                let status = if summary.successes > 0 {
                    ProbeStatus::Succeeded
                } else {
                    ProbeStatus::Failed
                };
                let detail = format_series_detail(&summary);
                ledger.set_status(
                    &tcp_id,
                    status,
                    Some(detail.clone()),
                );
                tcp.push(WindowsTcpObservation {
                    interface: adapter.name.clone(),
                    source,
                    target,
                    status,
                    detail,
                    summary,
                });
            }
        }

        Ok(WindowsRecoverySnapshot {
            ledger,
            adapters,
            dns,
            tcp,
            https,
        })
    }
}

#[derive(Debug, Clone)]
struct DnsProbeSuccess {
    elapsed: Duration,
    response_bytes: usize,
    rcode: u8,
}

fn series_to_link(
    summary: &ProbeSeriesSummary,
    status: ProbeStatus,
    from: NodeId,
    to: NodeId,
    transport: Transport,
    last_success_age: Duration,
) -> LinkObservation {
    let succeeded =
        status == ProbeStatus::Succeeded && summary.successes > 0;
    let rtt = summary
        .p95_rtt
        .or(summary.median_rtt)
        .or(summary.min_rtt)
        .unwrap_or(Duration::ZERO);

    MeasuredPathEvidence {
        estimated_bitrate_bps: summary.observed_useful_bitrate_bps,
        loss_ppm: summary.loss_ppm,
        rtt,
        intermittent:
            summary.intermittent || summary.loss_ppm >= 300_000,
        succeeded,
    }
    .into_link_observation(
        from,
        to,
        transport,
        Reachability::Internet,
        last_success_age,
    )
}

fn format_series_detail(summary: &ProbeSeriesSummary) -> String {
    format!(
        "attempts={} successes={} loss_ppm={} useful_bytes={} useful_bps={} median_ms={} p95_ms={} longest_failure_run={} transitions={} intermittent={}",
        summary.attempts,
        summary.successes,
        summary.loss_ppm,
        summary.total_useful_bytes,
        summary.observed_useful_bitrate_bps,
        summary
            .median_rtt
            .map(|value| value.as_millis().to_string())
            .unwrap_or_else(|| "-".to_owned()),
        summary
            .p95_rtt
            .map(|value| value.as_millis().to_string())
            .unwrap_or_else(|| "-".to_owned()),
        summary.longest_failure_run,
        summary.state_transitions,
        summary.intermittent,
    )
}

fn record_address_family(
    ledger: &mut RecoveryLedger,
    adapter: &WindowsAdapterPath,
    kind: ProbeKind,
    configured: bool,
) {
    let family = match kind {
        ProbeKind::Ipv4 => "ipv4",
        ProbeKind::Ipv6 => "ipv6",
        _ => "ip",
    };
    let id = format!("{}:{family}:configured", adapter.name);

    ledger.register(&id, kind);
    ledger.set_status(
        &id,
        if configured {
            ProbeStatus::Succeeded
        } else {
            ProbeStatus::Failed
        },
        Some(format!(
            "configured={} gateway={} ipv4_metric={} ipv6_metric={}",
            configured,
            adapter.has_gateway,
            adapter.ipv4_metric,
            adapter.ipv6_metric,
        )),
    );
}

fn select_source(
    adapter: &WindowsAdapterPath,
    resolver: IpAddr,
) -> Option<IpAddr> {
    adapter.unicast.iter().copied().find(|source| {
        if source.is_ipv4() != resolver.is_ipv4() {
            return false;
        }

        match source {
            IpAddr::V4(address) => {
                !address.is_unspecified()
                    && !address.is_loopback()
                    && !address.is_multicast()
            }
            IpAddr::V6(address) => {
                usable_ipv6_source(*address)
            }
        }
    })
}

fn usable_ipv6_source(address: Ipv6Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && !address.is_unicast_link_local()
}

fn probe_dns_udp(
    source: IpAddr,
    resolver: IpAddr,
    timeout: Duration,
) -> io::Result<DnsProbeSuccess> {
    probe_dns_udp_to(source, SocketAddr::new(resolver, 53), timeout)
}

fn probe_dns_udp_to(
    source: IpAddr,
    destination: SocketAddr,
    timeout: Duration,
) -> io::Result<DnsProbeSuccess> {
    if source.is_ipv4() != destination.is_ipv4() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source and destination address families differ",
        ));
    }

    let socket = UdpSocket::bind(SocketAddr::new(source, 0))?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;

    let query = root_a_query();
    let started = Instant::now();
    socket.send_to(&query, destination)?;

    let mut response = [0_u8; 512];
    let (received, peer) = socket.recv_from(&mut response)?;

    if peer.ip() != destination.ip() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "DNS response came from an unexpected address",
        ));
    }

    if received < 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "DNS response is shorter than the DNS header",
        ));
    }

    let response_id = u16::from_be_bytes([response[0], response[1]]);
    if response_id != 0x5350 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "DNS transaction ID mismatch",
        ));
    }

    if response[2] & 0x80 == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "DNS packet is not a response",
        ));
    }

    Ok(DnsProbeSuccess {
        elapsed: started.elapsed(),
        response_bytes: received,
        rcode: response[3] & 0x0f,
    })
}

fn root_a_query() -> [u8; 17] {
    [
        0x53, 0x50,
        0x01, 0x00,
        0x00, 0x01,
        0x00, 0x00,
        0x00, 0x00,
        0x00, 0x00,
        0x00,
        0x00, 0x01,
        0x00, 0x01,
    ]
}

fn classify_probe_error(error: &io::Error) -> ProbeStatus {
    match error.kind() {
        io::ErrorKind::PermissionDenied => ProbeStatus::Blocked,
        io::ErrorKind::Unsupported => ProbeStatus::Unsupported,
        _ => ProbeStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectivity_core::Transport;
    use std::net::{Ipv4Addr, UdpSocket};
    use std::thread;

    fn adapter() -> WindowsAdapterPath {
        WindowsAdapterPath {
            name: "test".to_owned(),
            transport: Transport::Wifi,
            available: true,
            has_gateway: true,
            ipv4_metric: 25,
            ipv6_metric: 35,
            tx_bps: 1_000_000,
            rx_bps: 1_000_000,
            unicast: vec![
                "192.0.2.50".parse().unwrap(),
                "2001:db8::50".parse().unwrap(),
            ],
            dns_servers: vec![],
        }
    }

    #[test]
    fn source_selection_matches_address_family() {
        let adapter = adapter();

        assert_eq!(
            select_source(&adapter, "8.8.8.8".parse().unwrap()),
            Some("192.0.2.50".parse().unwrap()),
        );
        assert_eq!(
            select_source(
                &adapter,
                "2606:4700:4700::1111".parse().unwrap()
            ),
            Some("2001:db8::50".parse().unwrap()),
        );
    }

    #[test]
    fn link_local_ipv6_is_not_used_without_scope() {
        let mut adapter = adapter();
        adapter.unicast = vec!["fe80::1234".parse().unwrap()];

        assert_eq!(
            select_source(
                &adapter,
                "2606:4700:4700::1111".parse().unwrap()
            ),
            None
        );
    }

    #[test]
    fn interface_bound_dns_probe_validates_real_udp_response() {
        let server =
            UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let destination = server.local_addr().unwrap();

        let responder = thread::spawn(move || {
            let mut query = [0_u8; 512];
            let (len, peer) = server.recv_from(&mut query).unwrap();
            assert_eq!(len, 17);
            assert_eq!(&query[..2], &[0x53, 0x50]);

            let mut response = query[..len].to_vec();
            response[2] |= 0x80;
            server.send_to(&response, peer).unwrap();
        });

        let success = probe_dns_udp_to(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            destination,
            Duration::from_secs(2),
        )
        .unwrap();

        assert_eq!(success.response_bytes, 17);
        assert_eq!(success.rcode, 0);
        responder.join().unwrap();
    }
}
