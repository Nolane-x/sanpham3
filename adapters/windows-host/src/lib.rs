pub mod path;
pub mod recovery;

pub use path::{inventory_adapter_paths, WindowsAdapterPath};
pub use recovery::{
    WindowsDnsObservation, WindowsHttpsObservation, WindowsRecoveryProbe,
    WindowsRecoverySnapshot, WindowsTcpObservation,
};

use connectivity_core::{
    Capability, PermissionState, PlatformScanner, Transport,
};

#[derive(Debug, Default)]
pub struct WindowsScanner;

impl WindowsScanner {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformScanner for WindowsScanner {
    fn platform_name(&self) -> &'static str {
        "windows"
    }

    fn inventory(&mut self) -> Vec<Capability> {
        #[cfg(target_os = "windows")]
        {
            match windows_impl::inventory() {
                Ok(capabilities) => capabilities,
                Err(error) => vec![Capability {
                    name: "windows-adapter-inventory".to_owned(),
                    interface: None,
                    transport: Transport::Other,
                    available: false,
                    permission: PermissionState::Denied,
                    can_scan: false,
                    can_connect: false,
                    can_advertise: false,
                    can_relay: false,
                    can_bind_socket: false,
                    constraints: vec![format!("GetAdaptersAddresses={error}")],
                }],
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            vec![Capability {
                name: "windows-adapter-inventory".to_owned(),
                interface: None,
                transport: Transport::Other,
                available: false,
                permission: PermissionState::Unsupported,
                can_scan: false,
                can_connect: false,
                can_advertise: false,
                can_relay: false,
                can_bind_socket: false,
                constraints: vec![
                    "Windows IP Helper is unavailable on this platform"
                        .to_owned(),
                ],
            }]
        }
    }
}

#[cfg(any(target_os = "windows", test))]
fn classify_if_type(if_type: u32) -> Transport {
    match if_type {
        // IANA / Windows interface type assignments.
        6 => Transport::Ethernet,       // IF_TYPE_ETHERNET_CSMACD
        71 => Transport::Wifi,          // IF_TYPE_IEEE80211
        243 | 244 => Transport::Cellular, // IF_TYPE_WWANPP / WWANPP2
        23 | 131 => Transport::Tunnel,  // IF_TYPE_PPP / IF_TYPE_TUNNEL
        _ => Transport::Other,
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::classify_if_type;
    use connectivity_core::{Capability, PermissionState, Transport};
    use std::alloc::{alloc_zeroed, dealloc, Layout};
    use std::mem::align_of;
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
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    const INITIAL_BUFFER_BYTES: u32 = 15 * 1024;
    const MAX_ATTEMPTS: usize = 3;
    const MAX_ADAPTERS: usize = 4096;

    struct AlignedBuffer {
        pointer: NonNull<u8>,
        layout: Layout,
    }

    impl AlignedBuffer {
        fn new(size: usize) -> Result<Self, String> {
            let size = size.max(std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>());
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
            // SAFETY: the pointer was allocated with this exact layout and has
            // not been deallocated elsewhere.
            unsafe {
                dealloc(self.pointer.as_ptr(), self.layout);
            }
        }
    }

    pub fn inventory() -> Result<Vec<Capability>, String> {
        let mut requested_bytes = INITIAL_BUFFER_BYTES;

        for _ in 0..MAX_ATTEMPTS {
            let mut buffer = AlignedBuffer::new(requested_bytes as usize)?;
            let mut actual_bytes = u32::try_from(buffer.len())
                .map_err(|_| "adapter buffer exceeds u32".to_owned())?;

            let flags = GAA_FLAG_INCLUDE_ALL_INTERFACES
                | GAA_FLAG_INCLUDE_GATEWAYS
                | GAA_FLAG_INCLUDE_PREFIX;

            // SAFETY: buffer is writable, sufficiently aligned for
            // IP_ADAPTER_ADDRESSES_LH, and actual_bytes describes its size.
            // Windows owns no part of the buffer after this synchronous call.
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
                return unsafe { parse_adapter_list(buffer.as_adapter_ptr()) };
            }

            if result == ERROR_BUFFER_OVERFLOW.0 {
                requested_bytes = actual_bytes.max(requested_bytes + 1024);
                continue;
            }

            return Err(format!("Win32 error code {result}"));
        }

        Err("adapter inventory remained larger than the retry buffer".to_owned())
    }

    unsafe fn parse_adapter_list(
        first: *mut IP_ADAPTER_ADDRESSES_LH,
    ) -> Result<Vec<Capability>, String> {
        let mut current = first;
        let mut capabilities = Vec::new();
        let mut count = 0_usize;

        while !current.is_null() {
            count += 1;
            if count > MAX_ADAPTERS {
                return Err("adapter linked list exceeded safety bound".to_owned());
            }

            // SAFETY: current points into the live buffer returned by
            // GetAdaptersAddresses. The traversal is bounded above.
            let adapter = unsafe { &*current };

            let friendly_name = if adapter.FriendlyName.is_null() {
                format!("windows-adapter-{count}")
            } else {
                // SAFETY: FriendlyName is documented as a NUL-terminated
                // string that remains valid while the adapter buffer lives.
                unsafe { adapter.FriendlyName.to_string() }
                    .unwrap_or_else(|_| format!("windows-adapter-{count}"))
            };

            let transport = classify_if_type(adapter.IfType);
            let status = adapter.OperStatus;
            let available = status == IfOperStatusUp
                || status == IfOperStatusDormant
                || status == IfOperStatusUnknown;
            let has_unicast = !adapter.FirstUnicastAddress.is_null();
            let has_gateway = !adapter.FirstGatewayAddress.is_null();
            let has_dns = !adapter.FirstDnsServerAddress.is_null();
            let loopback = adapter.IfType == 24; // IF_TYPE_SOFTWARE_LOOPBACK

            capabilities.push(Capability {
                name: format!("net:{friendly_name}"),
                interface: Some(friendly_name),
                transport,
                available: available && !loopback,
                permission: PermissionState::Granted,
                // Native WLAN/Bluetooth scanning is a separate adapter lane.
                can_scan: false,
                can_connect: !loopback && transport != Transport::Other,
                can_advertise: false,
                can_relay: available && has_unicast && !loopback,
                can_bind_socket: available && has_unicast && !loopback,
                constraints: vec![
                    format!("if_type={}", adapter.IfType),
                    format!("oper_status={}", status.0),
                    format!("mtu={}", adapter.Mtu),
                    format!("tx_bps={}", adapter.TransmitLinkSpeed),
                    format!("rx_bps={}", adapter.ReceiveLinkSpeed),
                    format!("ipv4_metric={}", adapter.Ipv4Metric),
                    format!("ipv6_metric={}", adapter.Ipv6Metric),
                    format!("unicast={has_unicast}"),
                    format!("gateway={has_gateway}"),
                    format!("dns={has_dns}"),
                ],
            });

            current = adapter.Next;
        }

        capabilities.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(capabilities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_core_windows_interface_types() {
        assert_eq!(classify_if_type(6), Transport::Ethernet);
        assert_eq!(classify_if_type(71), Transport::Wifi);
        assert_eq!(classify_if_type(243), Transport::Cellular);
        assert_eq!(classify_if_type(244), Transport::Cellular);
        assert_eq!(classify_if_type(131), Transport::Tunnel);
        assert_eq!(classify_if_type(24), Transport::Other);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_inventory_returns_terminal_result() {
        let mut scanner = WindowsScanner::new();
        let capabilities = scanner.inventory();

        assert!(!capabilities.is_empty());
        assert!(capabilities.iter().all(|capability| {
            capability.permission != PermissionState::RequiresUserAction
        }));
    }
}
