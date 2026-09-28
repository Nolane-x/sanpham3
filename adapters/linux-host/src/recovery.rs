use crate::dns_probe::{probe_dns_udp, DnsProbeSuccess};
use crate::resolver::read_resolvers;
use crate::routes::{read_route_snapshot, RouteSnapshot};
use connectivity_core::{
    Capability, PermissionState, ProbeKind, ProbeStatus, RecoveryLedger,
    Transport,
};
use std::io;
use std::net::IpAddr;
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

#[derive(Debug)]
pub struct LinuxRecoverySnapshot {
    pub ledger: RecoveryLedger,
    pub routes: RouteSnapshot,
    pub resolvers: Vec<IpAddr>,
    pub dns: Vec<DnsObservation>,
}

pub struct LinuxRecoveryProbe {
    ipv4_routes: PathBuf,
    ipv6_routes: PathBuf,
    resolv_conf: PathBuf,
    dns_timeout: Duration,
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
        }
    }

    pub fn dns_timeout(mut self, timeout: Duration) -> Self {
        self.dns_timeout = timeout;
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
        })
    }
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
