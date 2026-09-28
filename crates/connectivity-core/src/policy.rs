use crate::graph::{ConnectivityGraph, Route};
use crate::model::NodeId;
use crate::scoring::TrafficClass;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeliveryMode {
    /// Ordinary application/web-style transport.
    Full,
    /// Reduced payloads and fewer round trips.
    Compact,
    /// Structured semantic state rather than presentation bytes.
    Semantic,
    /// Extremely small semantic queries/results.
    TinySemantic,
    /// Experimental sub-10-bit/s control / emergency operation.
    Emergency,
}

impl DeliveryMode {
    pub fn from_effective_bps(effective_bps: u64) -> Self {
        match effective_bps {
            1_000_000.. => Self::Full,
            100_000..=999_999 => Self::Compact,
            1_000..=99_999 => Self::Semantic,
            10..=999 => Self::TinySemantic,
            _ => Self::Emergency,
        }
    }

    pub fn experimental(self) -> bool {
        self == Self::Emergency
    }

    pub fn supports(self, class: TrafficClass) -> bool {
        match class {
            TrafficClass::Critical | TrafficClass::TinySemantic => true,
            TrafficClass::Interactive => matches!(
                self,
                Self::Full | Self::Compact | Self::Semantic
            ),
            TrafficClass::Bulk => {
                matches!(self, Self::Full | Self::Compact)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPathKind {
    LocalEgress,
    DirectInternet,
    PeerEgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanReason {
    BestLivePath,
    NoLiveEgress,
    MeteredPathDisallowed,
    TrafficTooHeavyForPath,
    LiveWaitBudgetExceeded,
}

#[derive(Debug, Clone)]
pub struct RecoveryTask {
    pub class: TrafficClass,
    /// Estimated complete on-wire bytes for the task, including protocol
    /// overhead. Callers should use measured baselines when available.
    pub estimated_wire_bytes: u64,
    /// Expected request/response round trips for the chosen protocol profile.
    pub estimated_round_trips: u16,
    pub allow_metered: bool,
    pub allow_delay_tolerant: bool,
    /// Optional policy bound for how long the app is willing to occupy a live
    /// path before preferring store/carry/forward.
    pub max_live_wait: Option<Duration>,
}

impl RecoveryTask {
    pub fn new(class: TrafficClass, estimated_wire_bytes: u64) -> Self {
        Self {
            class,
            estimated_wire_bytes,
            estimated_round_trips: 2,
            allow_metered: true,
            allow_delay_tolerant: true,
            max_live_wait: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveRecoveryPlan {
    pub mode: DeliveryMode,
    pub path_kind: RecoveryPathKind,
    pub route_nodes: Vec<NodeId>,
    pub effective_bps: u64,
    pub expected_completion: Duration,
    pub worst_loss_ppm: u32,
    pub total_rtt: Duration,
    pub intermittent_hops: u16,
    pub metered_hops: u16,
    pub estimated_wire_bytes: u64,
    pub route_cost: f64,
    pub experimental: bool,
}

#[derive(Debug, Clone)]
pub enum RecoveryPlan {
    Live(LiveRecoveryPlan),
    DelayTolerant {
        reason: PlanReason,
        estimated_wire_bytes: u64,
    },
    LocalOnly {
        reason: PlanReason,
    },
}

impl RecoveryPlan {
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live(_))
    }
}

pub fn plan_recovery(
    graph: &ConnectivityGraph,
    start: NodeId,
    task: &RecoveryTask,
) -> RecoveryPlan {
    let Some(route) = graph.best_egress_route(start, task.class) else {
        return fallback(task, PlanReason::NoLiveEgress);
    };

    if route.metered_hops > 0 && !task.allow_metered {
        return fallback(task, PlanReason::MeteredPathDisallowed);
    }

    let effective_bps = route.estimated_effective_bps();
    let mode = DeliveryMode::from_effective_bps(effective_bps);

    if !mode.supports(task.class) {
        return fallback(task, PlanReason::TrafficTooHeavyForPath);
    }

    let expected_completion = estimate_completion(
        task.estimated_wire_bytes,
        task.estimated_round_trips,
        effective_bps,
        &route,
    );

    if task
        .max_live_wait
        .is_some_and(|budget| expected_completion > budget)
    {
        return fallback(task, PlanReason::LiveWaitBudgetExceeded);
    }

    RecoveryPlan::Live(LiveRecoveryPlan {
        mode,
        path_kind: classify_path(&route),
        route_nodes: route.nodes.clone(),
        effective_bps,
        expected_completion,
        worst_loss_ppm: route.worst_loss_ppm,
        total_rtt: route.total_rtt,
        intermittent_hops: route.intermittent_hops,
        metered_hops: route.metered_hops,
        estimated_wire_bytes: task.estimated_wire_bytes,
        route_cost: route.total_cost,
        experimental: mode.experimental(),
    })
}

fn fallback(task: &RecoveryTask, reason: PlanReason) -> RecoveryPlan {
    if task.allow_delay_tolerant {
        RecoveryPlan::DelayTolerant {
            reason,
            estimated_wire_bytes: task.estimated_wire_bytes,
        }
    } else {
        RecoveryPlan::LocalOnly { reason }
    }
}

fn classify_path(route: &Route) -> RecoveryPathKind {
    if route.hop_count() == 0 {
        RecoveryPathKind::LocalEgress
    } else if route.peer_only_hops == 0 {
        RecoveryPathKind::DirectInternet
    } else {
        RecoveryPathKind::PeerEgress
    }
}

fn estimate_completion(
    wire_bytes: u64,
    round_trips: u16,
    effective_bps: u64,
    route: &Route,
) -> Duration {
    let latency = route
        .total_rtt
        .saturating_mul(u32::from(round_trips.max(1)));

    if effective_bps == u64::MAX {
        return latency;
    }

    let bits = u128::from(wire_bytes).saturating_mul(8);
    let nanos = bits
        .saturating_mul(1_000_000_000)
        .div_ceil(u128::from(effective_bps.max(1)));
    let serialization = duration_from_nanos_saturating(nanos);

    latency.saturating_add(serialization)
}

fn duration_from_nanos_saturating(nanos: u128) -> Duration {
    let seconds = nanos / 1_000_000_000;
    if seconds > u128::from(u64::MAX) {
        return Duration::MAX;
    }

    Duration::new(seconds as u64, (nanos % 1_000_000_000) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LinkObservation, LinkState, NodeProfile, Reachability, Transport,
    };

    fn link(
        from: NodeId,
        to: NodeId,
        reachability: Reachability,
        bitrate: u64,
        loss_ppm: u32,
        state: LinkState,
    ) -> LinkObservation {
        LinkObservation {
            from,
            to,
            transport: Transport::Wifi,
            reachability,
            state,
            estimated_bitrate_bps: bitrate,
            loss_ppm,
            rtt: Duration::from_millis(50),
            energy_cost: 10,
            metered: false,
            last_success_age: Duration::ZERO,
        }
    }

    #[test]
    fn high_rate_direct_path_uses_full_mode() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, false));
        graph.observe_link(link(
            1,
            2,
            Reachability::Internet,
            5_000_000,
            10_000,
            LinkState::Up,
        ));

        let task = RecoveryTask::new(TrafficClass::Interactive, 20_000);
        let plan = plan_recovery(&graph, 1, &task);

        let RecoveryPlan::Live(plan) = plan else {
            panic!("expected live plan");
        };

        assert_eq!(plan.mode, DeliveryMode::Full);
        assert_eq!(plan.path_kind, RecoveryPathKind::DirectInternet);
        assert!(!plan.experimental);
    }

    #[test]
    fn one_hop_peer_egress_is_not_mislabeled_as_direct_internet() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, false));
        graph.observe_link(link(
            1,
            2,
            Reachability::PeerOnly,
            1_000_000,
            0,
            LinkState::Up,
        ));

        let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
        let RecoveryPlan::Live(plan) = plan_recovery(&graph, 1, &task) else {
            panic!("expected live plan");
        };

        assert_eq!(plan.path_kind, RecoveryPathKind::PeerEgress);
    }

    #[test]
    fn weak_peer_route_degrades_tiny_query_to_tiny_semantic() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::local(2));
        graph.upsert_node(NodeProfile::egress(3, false));

        graph.observe_link(link(
            1,
            2,
            Reachability::PeerOnly,
            2_000_000,
            0,
            LinkState::Up,
        ));
        graph.observe_link(link(
            2,
            3,
            Reachability::PeerOnly,
            82,
            120_000,
            LinkState::Up,
        ));

        let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
        let plan = plan_recovery(&graph, 1, &task);

        let RecoveryPlan::Live(plan) = plan else {
            panic!("expected live plan");
        };

        assert_eq!(plan.path_kind, RecoveryPathKind::PeerEgress);
        assert_eq!(plan.mode, DeliveryMode::TinySemantic);
        assert!(plan.effective_bps >= 10);
        assert!(plan.expected_completion > Duration::from_secs(20));
    }

    #[test]
    fn one_bit_path_is_explicitly_experimental_emergency_mode() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, false));
        graph.observe_link(link(
            1,
            2,
            Reachability::Internet,
            1,
            0,
            LinkState::Up,
        ));

        let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
        let plan = plan_recovery(&graph, 1, &task);

        let RecoveryPlan::Live(plan) = plan else {
            panic!("expected live emergency plan");
        };

        assert_eq!(plan.mode, DeliveryMode::Emergency);
        assert!(plan.experimental);
        assert!(plan.expected_completion > Duration::from_secs(1_000));
    }

    #[test]
    fn bulk_on_tiny_path_is_queued_instead_of_pretending_to_be_web() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, false));
        graph.observe_link(link(
            1,
            2,
            Reachability::Internet,
            30,
            0,
            LinkState::Up,
        ));

        let task = RecoveryTask::new(TrafficClass::Bulk, 1_000_000);
        let plan = plan_recovery(&graph, 1, &task);

        assert!(matches!(
            plan,
            RecoveryPlan::DelayTolerant {
                reason: PlanReason::TrafficTooHeavyForPath,
                ..
            }
        ));
    }

    #[test]
    fn intermittent_loss_reduces_effective_rate_before_mode_selection() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, false));
        graph.observe_link(link(
            1,
            2,
            Reachability::Internet,
            100,
            400_000,
            LinkState::Intermittent,
        ));

        let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
        let plan = plan_recovery(&graph, 1, &task);

        let RecoveryPlan::Live(plan) = plan else {
            panic!("expected live plan");
        };

        assert_eq!(plan.effective_bps, 30);
        assert_eq!(plan.mode, DeliveryMode::TinySemantic);
    }

    #[test]
    fn completion_estimate_accounts_for_protocol_round_trips() {
        let route = Route {
            nodes: vec![1, 2],
            total_cost: 1.0,
            bottleneck_bps: 1_000,
            worst_loss_ppm: 0,
            total_rtt: Duration::from_secs(1),
            intermittent_hops: 0,
            metered_hops: 0,
            peer_only_hops: 0,
        };

        let one = estimate_completion(125, 1, 1_000, &route);
        let three = estimate_completion(125, 3, 1_000, &route);

        assert_eq!(one, Duration::from_secs(2));
        assert_eq!(three, Duration::from_secs(4));
    }

    #[test]
    fn max_live_wait_can_choose_dtn() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, false));
        graph.observe_link(link(
            1,
            2,
            Reachability::Internet,
            10,
            0,
            LinkState::Up,
        ));

        let mut task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
        task.max_live_wait = Some(Duration::from_secs(60));

        assert!(matches!(
            plan_recovery(&graph, 1, &task),
            RecoveryPlan::DelayTolerant {
                reason: PlanReason::LiveWaitBudgetExceeded,
                ..
            }
        ));
    }

    #[test]
    fn no_route_without_dtn_becomes_local_only() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));

        let mut task = RecoveryTask::new(TrafficClass::Critical, 32);
        task.allow_delay_tolerant = false;

        assert!(matches!(
            plan_recovery(&graph, 1, &task),
            RecoveryPlan::LocalOnly {
                reason: PlanReason::NoLiveEgress
            }
        ));
    }

    #[test]
    fn disallowed_metered_route_waits_for_later_path() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, true));

        let mut metered = link(
            1,
            2,
            Reachability::Internet,
            1_000_000,
            0,
            LinkState::Up,
        );
        metered.metered = true;
        graph.observe_link(metered);

        let mut task = RecoveryTask::new(TrafficClass::Bulk, 1_000_000);
        task.allow_metered = false;

        assert!(matches!(
            plan_recovery(&graph, 1, &task),
            RecoveryPlan::DelayTolerant {
                reason: PlanReason::MeteredPathDisallowed,
                ..
            }
        ));
    }
}
