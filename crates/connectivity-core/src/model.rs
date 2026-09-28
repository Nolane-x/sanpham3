use std::time::Duration;

pub type NodeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    Ethernet,
    Wifi,
    WifiDirect,
    WifiAware,
    BluetoothLe,
    Cellular,
    Satellite,
    Tunnel,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    LocalOnly,
    PeerOnly,
    Internet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Intermittent,
    Down,
}

#[derive(Debug, Clone)]
pub struct NodeProfile {
    pub id: NodeId,
    pub relay_allowed: bool,
    pub internet_egress: bool,
    pub metered_egress: bool,
}

impl NodeProfile {
    pub fn local(id: NodeId) -> Self {
        Self {
            id,
            relay_allowed: true,
            internet_egress: false,
            metered_egress: false,
        }
    }

    pub fn egress(id: NodeId, metered: bool) -> Self {
        Self {
            id,
            relay_allowed: true,
            internet_egress: true,
            metered_egress: metered,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinkObservation {
    pub from: NodeId,
    pub to: NodeId,
    pub transport: Transport,
    pub reachability: Reachability,
    pub state: LinkState,
    pub estimated_bitrate_bps: u64,
    /// Packet loss in parts per million (0..=1_000_000).
    pub loss_ppm: u32,
    pub rtt: Duration,
    /// Relative energy cost from 0 (cheap) to 100 (expensive).
    pub energy_cost: u8,
    pub metered: bool,
    /// Age of the latest successful observation.
    pub last_success_age: Duration,
}

impl LinkObservation {
    pub fn usable(&self) -> bool {
        self.state != LinkState::Down
            && self.loss_ppm < 1_000_000
            && self.estimated_bitrate_bps > 0
    }
}
