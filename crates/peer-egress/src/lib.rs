use peer_session::{SecureSession, SessionError};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

pub const KIND_RESOLVE_REQUEST: u8 = 0x20;
pub const KIND_RESOLVE_RESPONSE: u8 = 0x21;
pub const MAX_HOSTNAME_LEN: usize = 253;
pub const MAX_RESULT_ADDRESSES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRequest {
    pub request_id: u32,
    pub hostname: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResolveStatus {
    Ok = 0,
    InvalidHostname = 1,
    ResolutionFailed = 2,
    NoPublicAddress = 3,
    ProtocolError = 4,
}

impl TryFrom<u8> for ResolveStatus {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Ok),
            1 => Ok(Self::InvalidHostname),
            2 => Ok(Self::ResolutionFailed),
            3 => Ok(Self::NoPublicAddress),
            4 => Ok(Self::ProtocolError),
            _ => Err(ProtocolError::InvalidStatus(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveResponse {
    pub request_id: u32,
    pub status: ResolveStatus,
    pub addresses: Vec<IpAddr>,
}

#[derive(Debug)]
pub enum PeerEgressError {
    Session(SessionError),
    Protocol(ProtocolError),
    Remote(ResolveStatus),
    RequestIdMismatch { expected: u32, got: u32 },
    UnexpectedFrameKind(u8),
}

impl From<SessionError> for PeerEgressError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

impl From<ProtocolError> for PeerEgressError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Truncated,
    InvalidUtf8,
    InvalidHostname,
    InvalidStatus(u8),
    InvalidAddressFamily(u8),
    AddressCountTooLarge,
    TrailingBytes,
}

pub trait Resolver {
    fn resolve(&self, hostname: &str) -> io::Result<Vec<IpAddr>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve(&self, hostname: &str) -> io::Result<Vec<IpAddr>> {
        let mut addresses = Vec::new();

        for socket in (hostname, 0).to_socket_addrs()? {
            let ip = socket.ip();
            if !addresses.contains(&ip) {
                addresses.push(ip);
            }
        }

        Ok(addresses)
    }
}

pub fn resolve_via_peer<S: Read + Write>(
    session: &mut SecureSession,
    stream: &mut S,
    request_id: u32,
    hostname: &str,
) -> Result<Vec<IpAddr>, PeerEgressError> {
    let request = ResolveRequest {
        request_id,
        hostname: hostname.to_owned(),
    };
    let payload = encode_request(&request)?;
    session.send(stream, KIND_RESOLVE_REQUEST, &payload)?;

    let (kind, payload) = session.receive(stream)?;
    if kind != KIND_RESOLVE_RESPONSE {
        return Err(PeerEgressError::UnexpectedFrameKind(kind));
    }

    let response = decode_response(&payload)?;
    if response.request_id != request_id {
        return Err(PeerEgressError::RequestIdMismatch {
            expected: request_id,
            got: response.request_id,
        });
    }

    if response.status != ResolveStatus::Ok {
        return Err(PeerEgressError::Remote(response.status));
    }

    Ok(response.addresses)
}

pub fn serve_one<S: Read + Write, R: Resolver>(
    session: &mut SecureSession,
    stream: &mut S,
    resolver: &R,
) -> Result<(), PeerEgressError> {
    let (kind, payload) = session.receive(stream)?;
    if kind != KIND_RESOLVE_REQUEST {
        return Err(PeerEgressError::UnexpectedFrameKind(kind));
    }

    let response = match decode_request(&payload) {
        Ok(request) => handle_request(request, resolver),
        Err(ProtocolError::InvalidHostname) => {
            let request_id = payload
                .get(0..4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_be_bytes)
                .unwrap_or(0);

            ResolveResponse {
                request_id,
                status: ResolveStatus::InvalidHostname,
                addresses: Vec::new(),
            }
        }
        Err(error) => return Err(PeerEgressError::Protocol(error)),
    };

    let payload = encode_response(&response)?;
    session.send(stream, KIND_RESOLVE_RESPONSE, &payload)?;
    Ok(())
}

pub fn handle_request<R: Resolver>(
    request: ResolveRequest,
    resolver: &R,
) -> ResolveResponse {
    if validate_public_hostname(&request.hostname).is_err() {
        return ResolveResponse {
            request_id: request.request_id,
            status: ResolveStatus::InvalidHostname,
            addresses: Vec::new(),
        };
    }

    let addresses = match resolver.resolve(&request.hostname) {
        Ok(addresses) => addresses,
        Err(_) => {
            return ResolveResponse {
                request_id: request.request_id,
                status: ResolveStatus::ResolutionFailed,
                addresses: Vec::new(),
            };
        }
    };

    let mut public = Vec::new();
    for address in addresses {
        if is_public_destination(address) && !public.contains(&address) {
            public.push(address);
            if public.len() == MAX_RESULT_ADDRESSES {
                break;
            }
        }
    }

    if public.is_empty() {
        ResolveResponse {
            request_id: request.request_id,
            status: ResolveStatus::NoPublicAddress,
            addresses: Vec::new(),
        }
    } else {
        ResolveResponse {
            request_id: request.request_id,
            status: ResolveStatus::Ok,
            addresses: public,
        }
    }
}

pub fn encode_request(
    request: &ResolveRequest,
) -> Result<Vec<u8>, ProtocolError> {
    validate_public_hostname(&request.hostname)?;

    let hostname = request.hostname.as_bytes();
    if hostname.len() > u8::MAX as usize {
        return Err(ProtocolError::InvalidHostname);
    }

    let mut out = Vec::with_capacity(5 + hostname.len());
    out.extend_from_slice(&request.request_id.to_be_bytes());
    out.push(hostname.len() as u8);
    out.extend_from_slice(hostname);
    Ok(out)
}

pub fn decode_request(bytes: &[u8]) -> Result<ResolveRequest, ProtocolError> {
    if bytes.len() < 5 {
        return Err(ProtocolError::Truncated);
    }

    let request_id = u32::from_be_bytes(
        bytes[0..4]
            .try_into()
            .map_err(|_| ProtocolError::Truncated)?,
    );
    let hostname_len = bytes[4] as usize;

    if bytes.len() != 5 + hostname_len {
        return Err(ProtocolError::Truncated);
    }

    let hostname = std::str::from_utf8(&bytes[5..])
        .map_err(|_| ProtocolError::InvalidUtf8)?
        .to_owned();

    validate_public_hostname(&hostname)?;

    Ok(ResolveRequest {
        request_id,
        hostname,
    })
}

pub fn encode_response(
    response: &ResolveResponse,
) -> Result<Vec<u8>, ProtocolError> {
    if response.addresses.len() > MAX_RESULT_ADDRESSES {
        return Err(ProtocolError::AddressCountTooLarge);
    }

    let mut out = Vec::with_capacity(6 + response.addresses.len() * 17);
    out.extend_from_slice(&response.request_id.to_be_bytes());
    out.push(response.status as u8);
    out.push(response.addresses.len() as u8);

    for address in &response.addresses {
        match address {
            IpAddr::V4(address) => {
                out.push(4);
                out.extend_from_slice(&address.octets());
            }
            IpAddr::V6(address) => {
                out.push(6);
                out.extend_from_slice(&address.octets());
            }
        }
    }

    Ok(out)
}

pub fn decode_response(
    bytes: &[u8],
) -> Result<ResolveResponse, ProtocolError> {
    if bytes.len() < 6 {
        return Err(ProtocolError::Truncated);
    }

    let request_id = u32::from_be_bytes(
        bytes[0..4]
            .try_into()
            .map_err(|_| ProtocolError::Truncated)?,
    );
    let status = ResolveStatus::try_from(bytes[4])?;
    let count = bytes[5] as usize;

    if count > MAX_RESULT_ADDRESSES {
        return Err(ProtocolError::AddressCountTooLarge);
    }

    let mut cursor = 6;
    let mut addresses = Vec::with_capacity(count);

    for _ in 0..count {
        let family = *bytes.get(cursor).ok_or(ProtocolError::Truncated)?;
        cursor += 1;

        let address = match family {
            4 => {
                let octets: [u8; 4] = bytes
                    .get(cursor..cursor + 4)
                    .ok_or(ProtocolError::Truncated)?
                    .try_into()
                    .map_err(|_| ProtocolError::Truncated)?;
                cursor += 4;
                IpAddr::V4(Ipv4Addr::from(octets))
            }
            6 => {
                let octets: [u8; 16] = bytes
                    .get(cursor..cursor + 16)
                    .ok_or(ProtocolError::Truncated)?
                    .try_into()
                    .map_err(|_| ProtocolError::Truncated)?;
                cursor += 16;
                IpAddr::V6(Ipv6Addr::from(octets))
            }
            other => return Err(ProtocolError::InvalidAddressFamily(other)),
        };

        addresses.push(address);
    }

    if cursor != bytes.len() {
        return Err(ProtocolError::TrailingBytes);
    }

    Ok(ResolveResponse {
        request_id,
        status,
        addresses,
    })
}

pub fn validate_public_hostname(hostname: &str) -> Result<(), ProtocolError> {
    if hostname.is_empty()
        || hostname.len() > MAX_HOSTNAME_LEN
        || !hostname.is_ascii()
        || hostname.parse::<IpAddr>().is_ok()
    {
        return Err(ProtocolError::InvalidHostname);
    }

    let normalized = hostname.trim_end_matches('.');
    if normalized.is_empty() || !normalized.contains('.') {
        return Err(ProtocolError::InvalidHostname);
    }

    let lower = normalized.to_ascii_lowercase();
    const BLOCKED_SUFFIXES: [&str; 7] = [
        ".local",
        ".localhost",
        ".internal",
        ".home",
        ".lan",
        ".localdomain",
        ".invalid",
    ];

    if BLOCKED_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix))
    {
        return Err(ProtocolError::InvalidHostname);
    }

    for label in normalized.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(ProtocolError::InvalidHostname);
        }

        let bytes = label.as_bytes();
        if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') {
            return Err(ProtocolError::InvalidHostname);
        }

        if !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        {
            return Err(ProtocolError::InvalidHostname);
        }
    }

    Ok(())
}

pub fn is_public_destination(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_v4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_v4(mapped);
            }
            is_public_v6(address)
        }
    }
}

fn is_public_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();

    if address.is_unspecified()
        || address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
    {
        return false;
    }

    if octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240
    {
        return false;
    }

    true
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    if address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || address.is_unique_local()
        || address.is_unicast_link_local()
    {
        return false;
    }

    let segments = address.segments();

    // 2001:db8::/32 documentation range.
    if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeResolver {
        result: io::Result<Vec<IpAddr>>,
    }

    impl Resolver for FakeResolver {
        fn resolve(&self, _hostname: &str) -> io::Result<Vec<IpAddr>> {
            match &self.result {
                Ok(addresses) => Ok(addresses.clone()),
                Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
            }
        }
    }

    #[test]
    fn request_roundtrip() {
        let request = ResolveRequest {
            request_id: 42,
            hostname: "example.com".to_owned(),
        };

        let encoded = encode_request(&request).unwrap();
        assert_eq!(decode_request(&encoded).unwrap(), request);
    }

    #[test]
    fn response_roundtrip_mixed_families() {
        let response = ResolveResponse {
            request_id: 9,
            status: ResolveStatus::Ok,
            addresses: vec![
                "93.184.216.34".parse().unwrap(),
                "2606:2800:220:1:248:1893:25c8:1946".parse().unwrap(),
            ],
        };

        let encoded = encode_response(&response).unwrap();
        assert_eq!(decode_response(&encoded).unwrap(), response);
    }

    #[test]
    fn blocks_local_and_literal_targets() {
        for hostname in [
            "localhost",
            "router.local",
            "printer.lan",
            "10.0.0.1",
            "::1",
            "singlelabel",
            "-bad.example",
            "bad-.example",
        ] {
            assert!(
                validate_public_hostname(hostname).is_err(),
                "{hostname} should be rejected"
            );
        }

        assert!(validate_public_hostname("example.com").is_ok());
    }

    #[test]
    fn filters_non_public_results() {
        let resolver = FakeResolver {
            result: Ok(vec![
                "127.0.0.1".parse().unwrap(),
                "10.0.0.1".parse().unwrap(),
                "169.254.1.1".parse().unwrap(),
                "100.64.0.1".parse().unwrap(),
                "192.0.2.1".parse().unwrap(),
                "8.8.8.8".parse().unwrap(),
                "2001:db8::1".parse().unwrap(),
                "2606:4700:4700::1111".parse().unwrap(),
            ]),
        };

        let response = handle_request(
            ResolveRequest {
                request_id: 1,
                hostname: "example.com".to_owned(),
            },
            &resolver,
        );

        assert_eq!(response.status, ResolveStatus::Ok);
        assert_eq!(
            response.addresses,
            vec![
                "8.8.8.8".parse::<IpAddr>().unwrap(),
                "2606:4700:4700::1111".parse::<IpAddr>().unwrap(),
            ]
        );
    }

    #[test]
    fn no_public_addresses_is_explicit() {
        let resolver = FakeResolver {
            result: Ok(vec!["192.168.1.1".parse().unwrap()]),
        };

        let response = handle_request(
            ResolveRequest {
                request_id: 5,
                hostname: "example.com".to_owned(),
            },
            &resolver,
        );

        assert_eq!(response.status, ResolveStatus::NoPublicAddress);
        assert!(response.addresses.is_empty());
    }


    #[test]
    fn encrypted_tcp_peer_resolves_through_mock_egress() {
        use peer_session::{
            perform_client_handshake, perform_server_handshake,
            NonceReplayCache, PeerKey,
        };
        use std::net::{TcpListener, TcpStream};
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let key = PeerKey::new([0x77; 32]);
            let mut replay = NonceReplayCache::new(16);
            let (_, mut session) =
                perform_server_handshake(&mut stream, 200, &key, &mut replay)
                    .unwrap();

            let resolver = FakeResolver {
                result: Ok(vec![
                    "10.0.0.7".parse().unwrap(),
                    "93.184.216.34".parse().unwrap(),
                ]),
            };

            serve_one(&mut session, &mut stream, &resolver).unwrap();
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let key = PeerKey::new([0x77; 32]);
        let (_, mut session) =
            perform_client_handshake(&mut stream, 100, &key).unwrap();

        let addresses = resolve_via_peer(
            &mut session,
            &mut stream,
            1234,
            "example.com",
        )
        .unwrap();

        assert_eq!(
            addresses,
            vec!["93.184.216.34".parse::<IpAddr>().unwrap()]
        );

        server.join().unwrap();
    }

    #[test]
    fn resolution_failure_is_explicit() {
        let resolver = FakeResolver {
            result: Err(io::Error::new(
                io::ErrorKind::NotFound,
                "resolver failure",
            )),
        };

        let response = handle_request(
            ResolveRequest {
                request_id: 7,
                hostname: "example.com".to_owned(),
            },
            &resolver,
        );

        assert_eq!(response.status, ResolveStatus::ResolutionFailed);
    }
}
