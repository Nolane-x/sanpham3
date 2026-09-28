use crate::{
    ConnectivityGraph, LinkObservation, LinkState, NodeId, NodeProfile,
    Reachability, Transport,
};
use std::time::Duration;

/// A path that has produced application-layer Internet evidence.
///
/// Adapters should construct this only after a probe that actually exchanged
/// authenticated/validated remote application bytes (currently tiny HTTPS).
/// DNS configuration, routes, carrier state, or a successful local TCP connect
/// alone are intentionally insufficient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasuredInternetPath {
    pub path_id: String,
    pub transport: Transport,
    pub state: LinkState,
    pub estimated_bitrate_bps: u64,
    pub loss_ppm: u32,
    /// Best current latency surrogate from the verified probe series.
    pub rtt: Duration,
    pub energy_cost: u8,
    pub metered: bool,
    pub last_success_age: Duration,
}

impl MeasuredInternetPath {
    pub fn new(
        path_id: impl Into<String>,
        transport: Transport,
        state: LinkState,
        estimated_bitrate_bps: u64,
        loss_ppm: u32,
        rtt: Duration,
    ) -> Self {
        Self {
            path_id: path_id.into(),
            transport,
            state,
            estimated_bitrate_bps: estimated_bitrate_bps.max(1),
            loss_ppm: loss_ppm.min(1_000_000),
            rtt,
            energy_cost: default_energy_cost(transport),
            metered: default_metered(transport),
            last_success_age: Duration::ZERO,
        }
    }

    pub fn as_link(&self, from: NodeId, to: NodeId) -> LinkObservation {
        LinkObservation {
            from,
            to,
            transport: self.transport,
            reachability: Reachability::Internet,
            state: self.state,
            estimated_bitrate_bps: self.estimated_bitrate_bps,
            loss_ppm: self.loss_ppm,
            rtt: self.rtt,
            energy_cost: self.energy_cost,
            metered: self.metered,
            last_success_age: self.last_success_age,
        }
    }
}

/// Installs one verified local Internet path into the graph.
///
/// A distinct synthetic egress node should be supplied for each independently
/// measured path. This keeps multiple adapters available to the route scorer
/// instead of collapsing them into the operating system's default route.
pub fn install_measured_internet_path(
    graph: &mut ConnectivityGraph,
    local_node: NodeId,
    egress_node: NodeId,
    path: &MeasuredInternetPath,
) {
    graph.upsert_node(NodeProfile::egress(egress_node, path.metered));
    graph.observe_link(path.as_link(local_node, egress_node));
}

pub fn default_energy_cost(transport: Transport) -> u8 {
    match transport {
        Transport::Ethernet => 10,
        Transport::Wifi => 20,
        Transport::WifiDirect | Transport::WifiAware => 25,
        Transport::BluetoothLe => 15,
        Transport::Cellular => 60,
        Transport::Satellite => 90,
        Transport::Tunnel => 35,
        Transport::Other => 50,
    }
}

pub fn default_metered(transport: Transport) -> bool {
    matches!(transport, Transport::Cellular | Transport::Satellite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        plan_recovery, DeliveryMode, RecoveryPathKind, RecoveryPlan,
        RecoveryTask, TrafficClass,
    };

    #[test]
    fn verified_measurement_drives_planner_without_handwritten_link() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));

        let path = MeasuredInternetPath::new(
            "wifi:verified",
            Transport::Wifi,
            LinkState::Intermittent,
            100,
            400_000,
            Duration::from_millis(250),
        );
        install_measured_internet_path(&mut graph, 1, 10_001, &path);

        let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
        let RecoveryPlan::Live(plan) = plan_recovery(&graph, 1, &task) else {
            panic!("verified path should produce a live plan");
        };

        // 100 bit/s * 60% delivery * 50% intermittent penalty = 30 bit/s.
        assert_eq!(plan.effective_bps, 30);
        assert_eq!(plan.mode, DeliveryMode::TinySemantic);
        assert_eq!(plan.path_kind, RecoveryPathKind::DirectInternet);
    }

    #[test]
    fn cellular_defaults_to_metered_and_higher_energy_cost() {
        let path = MeasuredInternetPath::new(
            "cell:0",
            Transport::Cellular,
            LinkState::Up,
            50_000,
            0,
            Duration::from_millis(100),
        );

        assert!(path.metered);
        assert!(path.energy_cost > default_energy_cost(Transport::Wifi));
    }

    #[test]
    fn constructor_clamps_invalid_extremes_conservatively() {
        let path = MeasuredInternetPath::new(
            "weak",
            Transport::Other,
            LinkState::Up,
            0,
            2_000_000,
            Duration::ZERO,
        );

        assert_eq!(path.estimated_bitrate_bps, 1);
        assert_eq!(path.loss_ppm, 1_000_000);
    }
}
