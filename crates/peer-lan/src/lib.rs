use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

const MAGIC: [u8; 4] = *b"SP3P";
const VERSION: u8 = 0;
pub const BEACON_LEN: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerBeacon {
    pub node_id: u64,
    pub relay_allowed: bool,
    pub internet_egress: bool,
    pub listen_port: u16,
    pub advertised_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeaconError {
    WrongLength,
    WrongMagic,
    UnsupportedVersion(u8),
}

impl PeerBeacon {
    pub fn encode(self) -> [u8; BEACON_LEN] {
        let mut out = [0_u8; BEACON_LEN];
        out[0..4].copy_from_slice(&MAGIC);
        out[4] = VERSION;

        let mut flags = 0_u8;
        if self.relay_allowed {
            flags |= 1;
        }
        if self.internet_egress {
            flags |= 1 << 1;
        }
        out[5] = flags;

        out[6..14].copy_from_slice(&self.node_id.to_be_bytes());
        out[14..16].copy_from_slice(&self.listen_port.to_be_bytes());
        out[16..20].copy_from_slice(&self.advertised_bps.to_be_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BeaconError> {
        if bytes.len() != BEACON_LEN {
            return Err(BeaconError::WrongLength);
        }
        if bytes[0..4] != MAGIC {
            return Err(BeaconError::WrongMagic);
        }
        if bytes[4] != VERSION {
            return Err(BeaconError::UnsupportedVersion(bytes[4]));
        }

        let flags = bytes[5];
        let node_id = u64::from_be_bytes(
            bytes[6..14]
                .try_into()
                .map_err(|_| BeaconError::WrongLength)?,
        );
        let listen_port = u16::from_be_bytes(
            bytes[14..16]
                .try_into()
                .map_err(|_| BeaconError::WrongLength)?,
        );
        let advertised_bps = u32::from_be_bytes(
            bytes[16..20]
                .try_into()
                .map_err(|_| BeaconError::WrongLength)?,
        );

        Ok(Self {
            node_id,
            relay_allowed: flags & 1 != 0,
            internet_egress: flags & (1 << 1) != 0,
            listen_port,
            advertised_bps,
        })
    }
}

/// Small UDP-broadcast discovery primitive for LAN experiments.
///
/// A received beacon is only an untrusted route hint. It does not authorize
/// relay use and must not be treated as authenticated identity.
pub struct LanDiscovery {
    socket: UdpSocket,
    discovery_port: u16,
}

impl LanDiscovery {
    pub fn bind(discovery_port: u16) -> io::Result<Self> {
        let address = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, discovery_port);
        let socket = UdpSocket::bind(address)?;
        socket.set_broadcast(true)?;

        Ok(Self {
            socket,
            discovery_port,
        })
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(timeout)
    }

    pub fn announce(&self, beacon: PeerBeacon) -> io::Result<usize> {
        let destination = SocketAddrV4::new(
            Ipv4Addr::BROADCAST,
            self.discovery_port,
        );
        self.socket.send_to(&beacon.encode(), destination)
    }

    pub fn receive(&self) -> io::Result<(PeerBeacon, SocketAddr)> {
        let mut bytes = [0_u8; BEACON_LEN];
        let (len, source) = self.socket.recv_from(&mut bytes)?;

        if len != BEACON_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected peer beacon length",
            ));
        }

        let beacon = PeerBeacon::decode(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid peer beacon: {error:?}"),
            )
        })?;

        Ok((beacon, source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beacon_roundtrip() {
        let beacon = PeerBeacon {
            node_id: 0x1122_3344_5566_7788,
            relay_allowed: true,
            internet_egress: true,
            listen_port: 45123,
            advertised_bps: 82,
        };

        let encoded = beacon.encode();
        assert_eq!(encoded.len(), BEACON_LEN);
        assert_eq!(PeerBeacon::decode(&encoded).unwrap(), beacon);
    }

    #[test]
    fn rejects_other_protocols() {
        let mut encoded = PeerBeacon {
            node_id: 1,
            relay_allowed: true,
            internet_egress: false,
            listen_port: 1,
            advertised_bps: 1,
        }
        .encode();

        encoded[0] = b'X';
        assert_eq!(
            PeerBeacon::decode(&encoded),
            Err(BeaconError::WrongMagic)
        );
    }
}
