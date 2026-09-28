use connectivity_core::{ProbeKind, ProbeStatus, RecoveryLedger};
#[cfg(target_os = "windows")]
use connectivity_core::Transport;
use std::io;
use std::net::IpAddr;
#[cfg(target_os = "windows")]
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;
#[cfg(target_os = "windows")]
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct WindowsDnsObservation {
    pub interface: String,
    pub source: IpAddr,
    pub resolver: IpAddr,
    pub status: ProbeStatus,
    pub detail: String,
}

#[derive(Debug)]
pub struct WindowsRecoverySnapshot {
    pub ledger: RecoveryLedger,
    pub dns: Vec<WindowsDnsObservation>,
}

#[derive(Debug, Clone)]
pub struct WindowsRecoveryProbe {
    dns_timeout: Duration,
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
        }
    }

    pub fn dns_timeout(mut self, timeout: Duration) -> Self {
        self.dns_timeout = timeout;
        self
    }

    pub fn run(&self) -> io::Result<WindowsRecoverySnapshot> {
        #[cfg(target_os = "windows")]
        {
            self.run_windows()
        }

        #[cfg(not(target_os = "windows"))]
        {
            let mut ledger = RecoveryLedger::new();
            ledger.register("windows:path-probe", ProbeKind::Other);
            ledger.set_status(
                "windows:path-probe",
                ProbeStatus::Unsupported,
                Some("Windows adapter probing is unavailable on this platform".to_owned()),
            );

            Ok(WindowsRecoverySnapshot {
                ledger,
                dns: Vec::new(),
            })
        }
    }

    #[cfg(target_os = "windows")]
    fn run_windows(&self) -> io::Result<WindowsRecoverySnapshot> {
        let interfaces = windows_impl::interface_snapshots()
            .map_err(io::Error::other)?;

        let mut ledger = RecoveryLedger::new();
        let mut dns = Vec::new();

        for interface in interfaces.into_iter().filter(|item| item.available) {
            record_family_configuration(
                &mut ledger,
                &interface.name,
                ProbeKind::Ipv4,
                interface.has_gateway
                    && interface.sources.iter().any(SocketAddr::is_ipv4),
            );
            record_family_configuration(
                &mut ledger,
                &interface.name,
                ProbeKind::Ipv6,
                interface.has_gateway
                    && interface.sources.iter().any(SocketAddr::is_ipv6),
            );

            let mut attempted = 0_usize;

            for source in &interface.sources {
                for resolver in &interface.dns_servers {
                    if source.is_ipv4() != resolver.is_ipv4() {
                        continue;
                    }

                    attempted += 1;
                    let id = format!(
                        "{}:dns:{}->{}",
                        interface.name,
                        source.ip(),
                        resolver.ip(),
                    );
                    ledger.register(&id, ProbeKind::Dns);

                    match probe_dns_udp(*source, *resolver, self.dns_timeout) {
                        Ok(success) => {
                            let detail = format!(
                                "source={} resolver={} elapsed_ms={} bytes={} rcode={}",
                                source.ip(),
                                resolver.ip(),
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
                                interface: interface.name.clone(),
                                source: source.ip(),
                                resolver: resolver.ip(),
                                status: ProbeStatus::Succeeded,
                                detail,
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
                                interface: interface.name.clone(),
                                source: source.ip(),
                                resolver: resolver.ip(),
                                status,
                                detail,
                            });
                        }
                    }
                }
            }

            if attempted == 0 {
                let id = format!("{}:dns", interface.name);
                ledger.register(&id, ProbeKind::Dns);
                ledger.set_status(
                    &id,
                    ProbeStatus::Unsupported,
                    Some(
                        "no source/DNS address-family pair exposed by adapter"
                            .to_owned(),
                    ),
                );
            }
        }

        Ok(WindowsRecoverySnapshot { ledger, dns })
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct DnsProbeSuccess {
    elapsed: Duration,
    response_bytes: usize,
    rcode: u8,
}

#[cfg(target_os = "windows")]
fn probe_dns_udp(
    source: SocketAddr,
    resolver: SocketAddr,
    timeout: Duration,
) -> io::Result<DnsProbeSuccess> {
    let bind_source = match source {
        SocketAddr::V4(mut address) => {
            address.set_port(0);
            SocketAddr::V4(address)
        }
        SocketAddr::V6(mut address) => {
            address.set_port(0);
            SocketAddr::V6(address)
        }
    };
    let target = match resolver {
        SocketAddr::V4(mut address) => {
            address.set_port(53);
            SocketAddr::V4(address)
        }
        SocketAddr::V6(mut address) => {
            address.set_port(53);
            SocketAddr::V6(address)
        }
    };

    let socket = UdpSocket::bind(bind_source)?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;
    socket.connect(target)?;

    let query = root_a_query();
    let started = Instant::now();
    socket.send(&query)?;

    let mut response = [0_u8; 512];
    let received = socket.recv(&mut response)?;

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
            "DNS response transaction ID mismatch",
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

#[cfg(any(target_os = "windows", test))]
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

#[cfg(target_os = "windows")]
fn record_family_configuration(
    ledger: &mut RecoveryLedger,
    interface: &str,
    kind: ProbeKind,
    configured: bool,
) {
    let family = match kind {
        ProbeKind::Ipv4 => "ipv4",
        ProbeKind::Ipv6 => "ipv6",
        _ => "ip",
    };
    let id = format!("{interface}:{family}:configured");

    ledger.register(&id, kind);
    ledger.set_status(
        &id,
        if configured {
            ProbeStatus::Succeeded
        } else {
            ProbeStatus::Failed
        },
        Some(if configured {
            "adapter exposes source address and gateway".to_owned()
        } else {
            "adapter lacks source address or gateway for this family".to_owned()
        }),
    );
}

#[cfg(target_os = "windows")]
fn classify_probe_error(error: &io::Error) -> ProbeStatus {
    match error.kind() {
        io::ErrorKind::PermissionDenied => ProbeStatus::Blocked,
        io::ErrorKind::Unsupported => ProbeStatus::Unsupported,
        _ => ProbeStatus::Failed,
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::Transport;
    use std::alloc::{alloc_zeroed, dealloc, Layout};
    use std::mem::align_of;
    use std::net::{
        Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6,
    };
    use std::ptr::NonNull;
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_ALL_INTERFACES,
        GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_INCLUDE_PREFIX,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::{
        IfOperStatusDormant, IfOperStatusUnknown, IfOperStatusUp,
    };
    use windows::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKET_ADDRESS,
    };

    const INITIAL_BUFFER_BYTES: u32 = 15 * 1024;
    const MAX_ATTEMPTS: usize = 3;
    const MAX_ADAPTERS: usize = 4096;
    const MAX_ADDRESSES_PER_LIST: usize = 256;

    #[derive(Debug)]
    pub(super) struct InterfaceSnapshot {
        pub name: String,
        pub available: bool,
        pub has_gateway: bool,
        pub sources: Vec<SocketAddr>,
        pub dns_servers: Vec<SocketAddr>,
    }

    struct AlignedBuffer {
        pointer: NonNull<u8>,
        layout: Layout,
    }

    impl AlignedBuffer {
        fn new(size: usize) -> Result<Self, String> {
            let size =
                size.max(std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>());
            let layout = Layout::from_size_align(
                size,
                align_of::<IP_ADAPTER_ADDRESSES_LH>(),
            )
            .map_err(|error| format!("invalid allocation layout: {error}"))?;

            // SAFETY: layout has non-zero size and valid alignment.
            let pointer = unsafe { alloc_zeroed(layout) };
            let pointer = NonNull::new(pointer)
                .ok_or_else(|| format!("allocation failed for {size} bytes"))?;

            Ok(Self { pointer, layout })
        }

        fn as_adapter_ptr(&mut self) -> *mut IP_ADAPTER_ADDRESSES_LH {
            self.pointer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()
        }

        fn len(&self) -> usize {
            self.layout.size()
        }
    }

    impl Drop for AlignedBuffer {
        fn drop(&mut self) {
            // SAFETY: pointer was allocated with this exact layout.
            unsafe {
                dealloc(self.pointer.as_ptr(), self.layout);
            }
        }
    }

    pub(super) fn interface_snapshots(
    ) -> Result<Vec<InterfaceSnapshot>, String> {
        let mut requested_bytes = INITIAL_BUFFER_BYTES;

        for _ in 0..MAX_ATTEMPTS {
            let mut buffer = AlignedBuffer::new(requested_bytes as usize)?;
            let mut actual_bytes = u32::try_from(buffer.len())
                .map_err(|_| "adapter buffer exceeds u32".to_owned())?;

            let flags = GAA_FLAG_INCLUDE_ALL_INTERFACES
                | GAA_FLAG_INCLUDE_GATEWAYS
                | GAA_FLAG_INCLUDE_PREFIX;

            // SAFETY: buffer is writable and correctly aligned.
            let result = unsafe {
                GetAdaptersAddresses(
                    AF_UNSPEC.0 as u32,
                    flags,
                    None,
                    Some(buffer.as_adapter_ptr()),
                    &mut actual_bytes,
                )
            };

            if result == 0 {
                // SAFETY: list lives inside buffer for this call.
                return unsafe {
                    parse_adapter_list(buffer.as_adapter_ptr())
                };
            }

            if result == ERROR_BUFFER_OVERFLOW.0 {
                requested_bytes =
                    actual_bytes.max(requested_bytes + 1024);
                continue;
            }

            return Err(format!("Win32 error code {result}"));
        }

        Err("adapter inventory remained larger than retry buffer".to_owned())
    }

    unsafe fn parse_adapter_list(
        first: *mut IP_ADAPTER_ADDRESSES_LH,
    ) -> Result<Vec<InterfaceSnapshot>, String> {
        let mut current = first;
        let mut snapshots = Vec::new();
        let mut count = 0_usize;

        while !current.is_null() {
            count += 1;
            if count > MAX_ADAPTERS {
                return Err(
                    "adapter linked list exceeded safety bound".to_owned(),
                );
            }

            // SAFETY: pointer belongs to GetAdaptersAddresses buffer.
            let adapter = unsafe { &*current };

            let name = if adapter.FriendlyName.is_null() {
                format!("windows-adapter-{count}")
            } else {
                // SAFETY: documented NUL-terminated name in live buffer.
                unsafe { adapter.FriendlyName.to_string() }
                    .unwrap_or_else(|_| format!("windows-adapter-{count}"))
            };

            let status = adapter.OperStatus;
            let available = status == IfOperStatusUp
                || status == IfOperStatusDormant
                || status == IfOperStatusUnknown;

            let mut sources = Vec::new();
            let mut unicast = adapter.FirstUnicastAddress;
            let mut address_count = 0_usize;

            while !unicast.is_null() {
                address_count += 1;
                if address_count > MAX_ADDRESSES_PER_LIST {
                    return Err(
                        "unicast list exceeded safety bound".to_owned(),
                    );
                }

                // SAFETY: node belongs to live adapter buffer.
                let node = unsafe { &*unicast };
                if let Some(address) =
                    unsafe { socket_address_to_std(&node.Address) }
                {
                    if !sources.contains(&address) {
                        sources.push(address);
                    }
                }
                unicast = node.Next;
            }

            let mut dns_servers = Vec::new();
            let mut dns = adapter.FirstDnsServerAddress;
            let mut dns_count = 0_usize;

            while !dns.is_null() {
                dns_count += 1;
                if dns_count > MAX_ADDRESSES_PER_LIST {
                    return Err(
                        "DNS server list exceeded safety bound".to_owned(),
                    );
                }

                // SAFETY: node belongs to live adapter buffer.
                let node = unsafe { &*dns };
                if let Some(address) =
                    unsafe { socket_address_to_std(&node.Address) }
                {
                    if !dns_servers.contains(&address) {
                        dns_servers.push(address);
                    }
                }
                dns = node.Next;
            }

            let transport = super::super::classify_if_type(adapter.IfType);
            let loopback = adapter.IfType == 24;

            if !loopback && transport != Transport::Other {
                snapshots.push(InterfaceSnapshot {
                    name,
                    available,
                    has_gateway: !adapter.FirstGatewayAddress.is_null(),
                    sources,
                    dns_servers,
                });
            }

            current = adapter.Next;
        }

        Ok(snapshots)
    }

    unsafe fn socket_address_to_std(
        address: &SOCKET_ADDRESS,
    ) -> Option<SocketAddr> {
        if address.lpSockaddr.is_null() || address.iSockaddrLength < 4 {
            return None;
        }

        let pointer = address.lpSockaddr.cast::<u8>();
        let length = usize::try_from(address.iSockaddrLength).ok()?;

        // SAFETY: SOCKET_ADDRESS guarantees iSockaddrLength readable bytes.
        let family = unsafe {
            u16::from_ne_bytes([
                *pointer,
                *pointer.add(1),
            ])
        };

        if family == AF_INET.0 as u16 && length >= 16 {
            let octets = unsafe {
                [
                    *pointer.add(4),
                    *pointer.add(5),
                    *pointer.add(6),
                    *pointer.add(7),
                ]
            };
            return Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from(octets),
                0,
            )));
        }

        if family == AF_INET6.0 as u16 && length >= 28 {
            let mut octets = [0_u8; 16];
            // SAFETY: sockaddr_in6 address occupies bytes 8..24.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    pointer.add(8),
                    octets.as_mut_ptr(),
                    octets.len(),
                );
            }

            let scope = unsafe {
                u32::from_ne_bytes([
                    *pointer.add(24),
                    *pointer.add(25),
                    *pointer.add(26),
                    *pointer.add(27),
                ])
            };

            return Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(octets),
                0,
                0,
                scope,
            )));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_dns_query_is_stable() {
        let query = root_a_query();
        assert_eq!(query.len(), 17);
        assert_eq!(&query[0..2], &[0x53, 0x50]);
        assert_eq!(&query[4..6], &[0x00, 0x01]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_windows_recovery_probe_returns_terminal_records() {
        let snapshot = WindowsRecoveryProbe::new()
            .dns_timeout(Duration::from_millis(150))
            .run()
            .unwrap();

        assert!(snapshot
            .ledger
            .records()
            .iter()
            .all(|record| record.status.terminal()));
    }
}
