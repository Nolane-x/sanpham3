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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportSchedulingDefaults {
    /// Relative scheduling weight only; not a physical energy measurement.
    pub energy_cost: u8,
    /// Conservative default when billing state is not yet known.
    pub metered: bool,
}

pub fn conservative_transport_defaults(
    transport: Transport,
) -> TransportSchedulingDefaults {
    match transport {
        Transport::Ethernet => TransportSchedulingDefaults {
            energy_cost: 5,
            metered: false,
        },
        Transport::Wifi => TransportSchedulingDefaults {
            energy_cost: 15,
            metered: false,
        },
        Transport::WifiDirect | Transport::WifiAware => {
            TransportSchedulingDefaults {
                energy_cost: 20,
                metered: false,
            }
        }
        Transport::BluetoothLe => TransportSchedulingDefaults {
            energy_cost: 10,
            metered: false,
        },
        Transport::Cellular => TransportSchedulingDefaults {
            energy_cost: 45,
            metered: true,
        },
        Transport::Satellite => TransportSchedulingDefaults {
            energy_cost: 80,
            metered: true,
        },
        Transport::Tunnel => TransportSchedulingDefaults {
            energy_cost: 25,
            metered: false,
        },
        Transport::Other => TransportSchedulingDefaults {
            energy_cost: 50,
            metered: false,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasuredPathEvidence {
    pub estimated_bitrate_bps: u64,
    pub loss_ppm: u32,
    pub rtt: Duration,
    pub intermittent: bool,
    pub succeeded: bool,
}

impl MeasuredPathEvidence {
    pub fn into_link_observation(
        self,
        from: NodeId,
        to: NodeId,
        transport: Transport,
        reachability: Reachability,
        last_success_age: Duration,
    ) -> LinkObservation {
        let defaults = conservative_transport_defaults(transport);

        LinkObservation {
            from,
            to,
            transport,
            reachability,
            state: if !self.succeeded {
                LinkState::Down
            } else if self.intermittent {
                LinkState::Intermittent
            } else {
                LinkState::Up
            },
            estimated_bitrate_bps: if self.succeeded {
                self.estimated_bitrate_bps.max(1)
            } else {
                0
            },
            loss_ppm: if self.succeeded {
                self.loss_ppm.min(999_999)
            } else {
                1_000_000
            },
            rtt: self.rtt,
            energy_cost: defaults.energy_cost,
            metered: defaults.metered,
            last_success_age,
        }
    }
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

#[cfg(test)]
mod measured_path_tests {
    use super::*;

    #[test]
    fn successful_intermittent_evidence_becomes_usable_link() {
        let link = MeasuredPathEvidence {
            estimated_bitrate_bps: 30,
            loss_ppm: 400_000,
            rtt: Duration::from_millis(900),
            intermittent: true,
            succeeded: true,
        }
        .into_link_observation(
            1,
            2,
            Transport::Wifi,
            Reachability::Internet,
            Duration::from_secs(1),
        );

        assert_eq!(link.state, LinkState::Intermittent);
        assert_eq!(link.estimated_bitrate_bps, 30);
        assert_eq!(link.loss_ppm, 400_000);
        assert!(link.usable());
        assert!(!link.metered);
    }

    #[test]
    fn failed_evidence_becomes_down_link() {
        let link = MeasuredPathEvidence {
            estimated_bitrate_bps: 999,
            loss_ppm: 0,
            rtt: Duration::from_secs(1),
            intermittent: false,
            succeeded: false,
        }
        .into_link_observation(
            1,
            2,
            Transport::Cellular,
            Reachability::Internet,
            Duration::from_secs(60),
        );

        assert_eq!(link.state, LinkState::Down);
        assert_eq!(link.estimated_bitrate_bps, 0);
        assert_eq!(link.loss_ppm, 1_000_000);
        assert!(link.metered);
        assert!(!link.usable());
    }

    #[test]
    fn conservative_defaults_are_explicit_policy_not_measurement() {
        assert_eq!(
            conservative_transport_defaults(Transport::Cellular),
            TransportSchedulingDefaults {
                energy_cost: 45,
                metered: true,
            }
        );
        assert_eq!(
            conservative_transport_defaults(Transport::Ethernet),
            TransportSchedulingDefaults {
                energy_cost: 5,
                metered: false,
            }
        );
    }
}
