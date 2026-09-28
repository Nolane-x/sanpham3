use connectivity_core::Transport;
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsAdapterPath {
    pub name: String,
    pub transport: Transport,
    pub available: bool,
    pub has_gateway: bool,
    pub ipv4_metric: u32,
    pub ipv6_metric: u32,
    pub tx_bps: u64,
    pub rx_bps: u64,
    pub unicast: Vec<IpAddr>,
    pub dns_servers: Vec<IpAddr>,
}

#[cfg(target_os = "windows")]
pub fn inventory_adapter_paths() -> Result<Vec<WindowsAdapterPath>, String> {
    windows_impl::inventory()
}

#[cfg(not(target_os = "windows"))]
pub fn inventory_adapter_paths() -> Result<Vec<WindowsAdapterPath>, String> {
    Err("Windows adapter path inventory is unavailable on this platform".to_owned())
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::WindowsAdapterPath;
    use crate::classify_if_type;
    use std::alloc::{alloc_zeroed, dealloc, Layout};
    use std::mem::align_of;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::ptr::NonNull;
    use std::slice;
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
    const MAX_LINKED_ADDRESSES: usize = 4096;

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
            // SAFETY: pointer and layout are the exact pair used to allocate.
            unsafe {
                dealloc(self.pointer.as_ptr(), self.layout);
            }
        }
    }

    pub fn inventory() -> Result<Vec<WindowsAdapterPath>, String> {
        let mut requested_bytes = INITIAL_BUFFER_BYTES;

        for _ in 0..MAX_ATTEMPTS {
            let mut buffer = AlignedBuffer::new(requested_bytes as usize)?;
            let mut actual_bytes = u32::try_from(buffer.len())
                .map_err(|_| "adapter buffer exceeds u32".to_owned())?;

            let flags = GAA_FLAG_INCLUDE_ALL_INTERFACES
                | GAA_FLAG_INCLUDE_GATEWAYS
                | GAA_FLAG_INCLUDE_PREFIX;

            // SAFETY: the buffer is writable/aligned and lives through the
            // synchronous GetAdaptersAddresses call and parsing below.
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
                // SAFETY: the buffer contains the linked list returned by the
                // successful call above and remains live for this parse.
                return unsafe { parse_adapter_list(buffer.as_adapter_ptr()) };
            }

            if result == ERROR_BUFFER_OVERFLOW.0 {
                requested_bytes = actual_bytes.max(requested_bytes + 1024);
                continue;
            }

            return Err(format!("GetAdaptersAddresses Win32 error {result}"));
        }

        Err("adapter inventory exceeded retry buffer".to_owned())
    }

    unsafe fn parse_adapter_list(
        first: *mut IP_ADAPTER_ADDRESSES_LH,
    ) -> Result<Vec<WindowsAdapterPath>, String> {
        let mut current = first;
        let mut paths = Vec::new();
        let mut count = 0_usize;

        while !current.is_null() {
            count += 1;
            if count > MAX_ADAPTERS {
                return Err("adapter linked list exceeded safety bound".to_owned());
            }

            // SAFETY: current points into the live GetAdaptersAddresses buffer.
            let adapter = unsafe { &*current };

            let name = if adapter.FriendlyName.is_null() {
                format!("windows-adapter-{count}")
            } else {
                // SAFETY: FriendlyName is a NUL-terminated string owned by the
                // live adapter buffer.
                unsafe { adapter.FriendlyName.to_string() }
                    .unwrap_or_else(|_| format!("windows-adapter-{count}"))
            };

            let status = adapter.OperStatus;
            let available = status == IfOperStatusUp
                || status == IfOperStatusDormant
                || status == IfOperStatusUnknown;
            let loopback = adapter.IfType == 24;

            let unicast =
                unsafe { collect_unicast(adapter.FirstUnicastAddress)? };
            let dns_servers =
                unsafe { collect_dns(adapter.FirstDnsServerAddress)? };

            paths.push(WindowsAdapterPath {
                name,
                transport: classify_if_type(adapter.IfType),
                available: available && !loopback,
                has_gateway: !adapter.FirstGatewayAddress.is_null(),
                ipv4_metric: adapter.Ipv4Metric,
                ipv6_metric: adapter.Ipv6Metric,
                tx_bps: adapter.TransmitLinkSpeed,
                rx_bps: adapter.ReceiveLinkSpeed,
                unicast,
                dns_servers,
            });

            current = adapter.Next;
        }

        paths.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(paths)
    }

    unsafe fn collect_unicast(
        mut current: *mut windows::Win32::NetworkManagement::IpHelper::IP_ADAPTER_UNICAST_ADDRESS_LH,
    ) -> Result<Vec<IpAddr>, String> {
        let mut addresses = Vec::new();
        let mut count = 0_usize;

        while !current.is_null() {
            count += 1;
            if count > MAX_LINKED_ADDRESSES {
                return Err("unicast address list exceeded safety bound".to_owned());
            }

            // SAFETY: current belongs to the adapter buffer.
            let entry = unsafe { &*current };
            if let Some(address) = unsafe { socket_address_ip(&entry.Address) } {
                if !addresses.contains(&address) {
                    addresses.push(address);
                }
            }
            current = entry.Next;
        }

        Ok(addresses)
    }

    unsafe fn collect_dns(
        mut current: *mut windows::Win32::NetworkManagement::IpHelper::IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
    ) -> Result<Vec<IpAddr>, String> {
        let mut addresses = Vec::new();
        let mut count = 0_usize;

        while !current.is_null() {
            count += 1;
            if count > MAX_LINKED_ADDRESSES {
                return Err("DNS address list exceeded safety bound".to_owned());
            }

            // SAFETY: current belongs to the adapter buffer.
            let entry = unsafe { &*current };
            if let Some(address) = unsafe { socket_address_ip(&entry.Address) } {
                if !addresses.contains(&address) {
                    addresses.push(address);
                }
            }
            current = entry.Next;
        }

        Ok(addresses)
    }

    unsafe fn socket_address_ip(address: &SOCKET_ADDRESS) -> Option<IpAddr> {
        if address.lpSockaddr.is_null() || address.iSockaddrLength <= 0 {
            return None;
        }

        let len = usize::try_from(address.iSockaddrLength).ok()?;
        // SAFETY: SOCKET_ADDRESS describes a live byte buffer of the declared
        // length owned by GetAdaptersAddresses.
        let bytes = unsafe {
            slice::from_raw_parts(
                address.lpSockaddr.cast::<u8>(),
                len,
            )
        };

        if bytes.len() < 2 {
            return None;
        }

        let family = u16::from_ne_bytes([bytes[0], bytes[1]]);

        if family == AF_INET.0 as u16 {
            if bytes.len() < 8 {
                return None;
            }

            return Some(IpAddr::V4(Ipv4Addr::new(
                bytes[4], bytes[5], bytes[6], bytes[7],
            )));
        }

        if family == AF_INET6.0 as u16 {
            if bytes.len() < 24 {
                return None;
            }

            let octets: [u8; 16] = bytes[8..24].try_into().ok()?;
            return Some(IpAddr::V6(Ipv6Addr::from(octets)));
        }

        None
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn native_structured_inventory_returns_paths() {
            let paths = inventory().expect("Windows path inventory");
            assert!(!paths.is_empty());

            assert!(paths.iter().any(|path| {
                !path.name.is_empty()
                    && (path.available
                        || !path.unicast.is_empty()
                        || !path.dns_servers.is_empty())
            }));
        }
    }
}
