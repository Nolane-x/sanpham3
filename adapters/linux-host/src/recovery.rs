use crate::dns_probe::{probe_dns_udp, DnsProbeSuccess};
use crate::resolver::read_resolvers;
use crate::routes::{read_route_snapshot, RouteSnapshot};
use connectivity_core::{
    Capability, PermissionState, ProbeKind, ProbeStatus, RecoveryLedger,
    Transport,
};
use std::io;
use path_probe::{
    run_series, summarize, tcp_connect_device, tiny_https_head_device,
    AttemptMeasurement, HttpsProbeTarget, ProbeSeriesSummary,
};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DnsObservation {
    pub interface: String,
    pub resolver: IpAddr,
    pub status: ProbeStatus,
    pub detail: String,
    pub success: Option<DnsProbeSuccess>,
}

#[derive(Debug, Clone)]
pub struct TcpSeriesObservation {
    pub interface: String,
    pub target: SocketAddr,
    pub status: ProbeStatus,
    pub detail: String,
    pub summary: ProbeSeriesSummary,
}

#[derive(Debug, Clone)]
pub struct HttpsSeriesObservation {
    pub interface: String,
    pub target: HttpsProbeTarget,
    pub status: ProbeStatus,
    pub detail: String,
    pub summary: ProbeSeriesSummary,
}

#[derive(Debug)]
pub struct LinuxRecoverySnapshot {
    pub ledger: RecoveryLedger,
    pub routes: RouteSnapshot,
    pub resolvers: Vec<IpAddr>,
    pub dns: Vec<DnsObservation>,
    pub tcp: Vec<TcpSeriesObservation>,
    pub https: Vec<HttpsSeriesObservation>,
}

pub struct LinuxRecoveryProbe {
    ipv4_routes: PathBuf,
    ipv6_routes: PathBuf,
    resolv_conf: PathBuf,
    dns_timeout: Duration,
    tcp_timeout: Duration,
    tcp_attempts: usize,
    tcp_pause: Duration,
    https_targets: Vec<HttpsProbeTarget>,
    https_timeout: Duration,
    https_attempts: usize,
    https_pause: Duration,
}

impl Default for LinuxRecoveryProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxRecoveryProbe {
    pub fn new() -> Self {
        Self {
            ipv4_routes: PathBuf::from("/proc/net/route"),
            ipv6_routes: PathBuf::from("/proc/net/ipv6_route"),
            resolv_conf: PathBuf::from("/etc/resolv.conf"),
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

    #[cfg(test)]
    fn with_paths(
        ipv4_routes: impl Into<PathBuf>,
        ipv6_routes: impl Into<PathBuf>,
        resolv_conf: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ipv4_routes: ipv4_routes.into(),
            ipv6_routes: ipv6_routes.into(),
            resolv_conf: resolv_conf.into(),
            dns_timeout: Duration::from_millis(50),
            tcp_timeout: Duration::from_millis(50),
            tcp_attempts: 1,
            tcp_pause: Duration::ZERO,
            https_targets: Vec::new(),
            https_timeout: Duration::from_millis(50),
            https_attempts: 1,
            https_pause: Duration::ZERO,
        }
    }

    pub fn run(
        &self,
        capabilities: &[Capability],
    ) -> io::Result<LinuxRecoverySnapshot> {
        let routes = read_route_snapshot(
            &self.ipv4_routes,
            &self.ipv6_routes,
        )?;
        let resolvers = read_resolvers(&self.resolv_conf).unwrap_or_default();
        let mut ledger = RecoveryLedger::new();
        let mut dns = Vec::new();
        let mut tcp = Vec::new();
        let mut https = Vec::new();

        for capability in capabilities.iter().filter(|capability| {
            capability.available
                && capability.permission == PermissionState::Granted
                && capability.can_connect
                && capability.transport != Transport::Other
        }) {
            let Some(interface) = capability.interface.as_deref() else {
                continue;
            };

            record_route_state(
                &mut ledger,
                interface,
                ProbeKind::Ipv4,
                routes.has_ipv4_default(interface),
            );
            record_route_state(
                &mut ledger,
                interface,
                ProbeKind::Ipv6,
                routes.has_ipv6_default(interface),
            );

            if self.https_targets.is_empty() {
                let id = format!("{interface}:https");
                ledger.register(&id, ProbeKind::TinyHttps);
                ledger.set_status(
                    &id,
                    ProbeStatus::Unsupported,
                    Some("no HTTPS probe target configured".to_owned()),
                );
            } else {
                for target in &self.https_targets {
                    let id = format!(
                        "{interface}:https:{}:{}",
                        target.server_name,
                        target.address,
                    );
                    ledger.register(&id, ProbeKind::TinyHttps);

                    let samples = run_series(
                        self.https_attempts,
                        self.https_pause,
                        || {
                            let result = tiny_https_head_device(
                                interface,
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
                    https.push(HttpsSeriesObservation {
                        interface: interface.to_owned(),
                        target: target.clone(),
                        status,
                        detail,
                        summary,
                    });
                }
            }

            let matching_resolvers = resolvers.iter().copied().filter(
                |resolver| {
                    (resolver.is_ipv4() && routes.has_ipv4_default(interface))
                        || (resolver.is_ipv6()
                            && routes.has_ipv6_default(interface))
                },
            );

            let mut resolver_count = 0_usize;

            for resolver in matching_resolvers {
                resolver_count += 1;
                let id = format!("{interface}:dns:{resolver}");
                ledger.register(&id, ProbeKind::Dns);

                let result =
                    probe_dns_udp(interface, resolver, self.dns_timeout);

                match result {
                    Ok(success) => {
                        let detail = format!(
                            "resolver={} elapsed_ms={} bytes={} rcode={}",
                            success.resolver,
                            success.elapsed.as_millis(),
                            success.response_bytes,
                            success.rcode,
                        );
                        ledger.set_status(
                            &id,
                            ProbeStatus::Succeeded,
                            Some(detail.clone()),
                        );
                        dns.push(DnsObservation {
                            interface: interface.to_owned(),
                            resolver,
                            status: ProbeStatus::Succeeded,
                            detail,
                            success: Some(success),
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
                        dns.push(DnsObservation {
                            interface: interface.to_owned(),
                            resolver,
                            status,
                            detail,
                            success: None,
                        });
                    }
                }

                let target = SocketAddr::new(resolver, 53);
                let tcp_id = format!("{interface}:tcp:{target}");
                ledger.register(&tcp_id, ProbeKind::Tcp);

                let samples = run_series(
                    self.tcp_attempts,
                    self.tcp_pause,
                    || {
                        let result = tcp_connect_device(
                            interface,
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
                tcp.push(TcpSeriesObservation {
                    interface: interface.to_owned(),
                    target,
                    status,
                    detail,
                    summary,
                });
            }

            if resolver_count == 0 {
                let id = format!("{interface}:dns");
                ledger.register(&id, ProbeKind::Dns);
                ledger.set_status(
                    &id,
                    ProbeStatus::Unsupported,
                    Some(
                        "no configured resolver matches this interface route family"
                            .to_owned(),
                    ),
                );
            }
        }

        Ok(LinuxRecoverySnapshot {
            ledger,
            routes,
            resolvers,
            dns,
            tcp,
            https,
        })
    }
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

fn record_route_state(
    ledger: &mut RecoveryLedger,
    interface: &str,
    kind: ProbeKind,
    present: bool,
) {
    let family = match kind {
        ProbeKind::Ipv4 => "ipv4",
        ProbeKind::Ipv6 => "ipv6",
        _ => "route",
    };
    let id = format!("{interface}:{family}:default-route");

    ledger.register(&id, kind);
    ledger.set_status(
        &id,
        if present {
            ProbeStatus::Succeeded
        } else {
            ProbeStatus::Failed
        },
        Some(if present {
            "kernel default route present".to_owned()
        } else {
            "kernel default route absent".to_owned()
        }),
    );
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
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "sanpham3-linux-recovery-{}-{suffix}",
            std::process::id()
        ))
    }

    fn capability(interface: &str) -> Capability {
        Capability {
            name: format!("net:{interface}"),
            interface: Some(interface.to_owned()),
            transport: Transport::Wifi,
            available: true,
            permission: PermissionState::Granted,
            can_scan: true,
            can_connect: true,
            can_advertise: false,
            can_relay: true,
            can_bind_socket: true,
            constraints: vec![],
        }
    }

    #[test]
    fn route_records_are_terminal_even_without_resolver() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();

        let v4 = root.join("route");
        let v6 = root.join("ipv6_route");
        let resolv = root.join("resolv.conf");

        fs::write(
            &v4,
            "Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT\n\
wlan0 00000000 0102A8C0 0003 0 0 600 00000000 0 0 0\n",
        )
        .unwrap();
        fs::write(&v6, "").unwrap();
        fs::write(&resolv, "").unwrap();

        let probe = LinuxRecoveryProbe::with_paths(v4, v6, resolv);
        let result = probe.run(&[capability("wlan0")]).unwrap();

        assert_eq!(result.ledger.pending_count(), 0);
        assert!(result
            .ledger
            .records()
            .iter()
            .any(|record| record.id == "wlan0:ipv4:default-route"
                && record.status == ProbeStatus::Succeeded));
        assert!(result
            .ledger
            .records()
            .iter()
            .any(|record| record.id == "wlan0:ipv6:default-route"
                && record.status == ProbeStatus::Failed));
        assert!(result
            .ledger
            .records()
            .iter()
            .any(|record| record.id == "wlan0:dns"
                && record.status == ProbeStatus::Unsupported));

        fs::remove_dir_all(root).unwrap();
    }
}
