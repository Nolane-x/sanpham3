#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CString;
    use std::io;
    use std::net::{IpAddr, SocketAddr, UdpSocket};
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_int, c_void};
    use std::time::{Duration, Instant};

    const SOL_SOCKET: c_int = 1;
    const SO_BINDTODEVICE: c_int = 25;

    unsafe extern "C" {
        fn setsockopt(
            socket: c_int,
            level: c_int,
            option_name: c_int,
            option_value: *const c_void,
            option_len: u32,
        ) -> c_int;
    }

    #[derive(Debug, Clone)]
    pub struct DnsProbeSuccess {
        pub resolver: IpAddr,
        pub elapsed: Duration,
        pub response_bytes: usize,
        pub rcode: u8,
    }

    pub fn probe_dns_udp(
        interface: &str,
        resolver: IpAddr,
        timeout: Duration,
    ) -> io::Result<DnsProbeSuccess> {
        let bind_addr = match resolver {
            IpAddr::V4(_) => "0.0.0.0:0",
            IpAddr::V6(_) => "[::]:0",
        };

        let socket = UdpSocket::bind(bind_addr)?;
        bind_socket_to_device(&socket, interface)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;

        let destination = SocketAddr::new(resolver, 53);
        let query = root_a_query();
        let started = Instant::now();

        socket.send_to(&query, destination)?;

        let mut response = [0_u8; 512];
        let (received, source) = socket.recv_from(&mut response)?;

        if source.ip() != resolver {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "DNS response came from an unexpected resolver",
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
            resolver,
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

    fn bind_socket_to_device(
        socket: &UdpSocket,
        interface: &str,
    ) -> io::Result<()> {
        let interface = CString::new(interface).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "interface contains an interior NUL byte",
            )
        })?;

        let bytes = interface.as_bytes_with_nul();

        // SAFETY: the file descriptor is owned by the UdpSocket and the
        // NUL-terminated interface buffer remains valid for the whole call.
        let result = unsafe {
            setsockopt(
                socket.as_raw_fd(),
                SOL_SOCKET,
                SO_BINDTODEVICE,
                bytes.as_ptr().cast::<c_void>(),
                bytes.len() as u32,
            )
        };

        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn query_is_small_and_structurally_valid() {
            let query = root_a_query();
            assert_eq!(query.len(), 17);
            assert_eq!(&query[0..2], &[0x53, 0x50]);
            assert_eq!(&query[4..6], &[0x00, 0x01]);
            assert_eq!(&query[12..], &[0x00, 0x00, 0x01, 0x00, 0x01]);
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(target_os = "linux"))]
mod unsupported {
    use std::io;
    use std::net::IpAddr;
    use std::time::Duration;

    #[derive(Debug, Clone)]
    pub struct DnsProbeSuccess {
        pub resolver: IpAddr,
        pub elapsed: Duration,
        pub response_bytes: usize,
        pub rcode: u8,
    }

    pub fn probe_dns_udp(
        _interface: &str,
        _resolver: IpAddr,
        _timeout: Duration,
    ) -> io::Result<DnsProbeSuccess> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Linux interface-bound DNS probe is unavailable on this platform",
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub use unsupported::*;
