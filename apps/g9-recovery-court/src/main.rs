use connectivity_core::{
    plan_recovery, ConnectivityGraph, LinkObservation, LinkState, NodeProfile,
    ProbeKind, ProbeStatus, Reachability, RecoveryLedger, RecoveryPathKind,
    RecoveryPlan, RecoveryTask, TrafficClass, Transport,
};
use peer_egress::{resolve_via_peer, serve_one, Resolver};
use peer_session::{
    perform_client_handshake, perform_server_handshake, NonceReplayCache,
    PeerKey,
};
use std::io;
use std::net::{IpAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const LOCAL_NODE: u64 = 100;
const DEAD_DEFAULT_EGRESS: u64 = 200;
const PEER_EGRESS_NODE: u64 = 300;
const REQUEST_ID: u32 = 9_001;
const HOSTNAME: &str = "example.com";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CourtEvidence {
    default_information_probe_failed: bool,
    selected_peer_path: bool,
    authenticated_peer_node: u64,
    returned_addresses: Vec<IpAddr>,
}

struct FixedResolver;

impl Resolver for FixedResolver {
    fn resolve(&self, _hostname: &str) -> io::Result<Vec<IpAddr>> {
        Ok(vec![
            "10.0.0.7".parse().unwrap(),
            "8.8.8.8".parse().unwrap(),
        ])
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("g9-recovery-court: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let evidence = run_software_court()?;

    println!(
        "G9_SOFTWARE_PASS default_failed={} selected_peer={} authenticated_peer={} addresses={:?}",
        evidence.default_information_probe_failed,
        evidence.selected_peer_path,
        evidence.authenticated_peer_node,
        evidence.returned_addresses,
    );

    println!(
        "NOTE software court only; physical G9 still requires a real OS/default-path failure and a real alternate/peer path"
    );

    Ok(())
}

fn run_software_court() -> Result<CourtEvidence, String> {
    let ledger = default_path_failure_ledger();
    if !ledger.exhaustive_complete() {
        return Err("default-path evidence is not terminal".to_owned());
    }
    if ledger.can_declare_local_only() {
        return Err(
            "peer discovery succeeded, so LOCAL_ONLY would be incorrect"
                .to_owned(),
        );
    }

    let graph = recovery_graph();
    let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
    let plan = plan_recovery(&graph, LOCAL_NODE, &task);

    let RecoveryPlan::Live(plan) = plan else {
        return Err("planner did not choose a live rescue path".to_owned());
    };

    if plan.path_kind != RecoveryPathKind::PeerEgress {
        return Err(format!(
            "planner selected {:?}, expected PeerEgress",
            plan.path_kind
        ));
    }
    if plan.route_nodes != vec![LOCAL_NODE, PEER_EGRESS_NODE] {
        return Err(format!(
            "unexpected rescue route {:?}",
            plan.route_nodes
        ));
    }

    let (authenticated_peer_node, returned_addresses) =
        resolve_through_authenticated_peer()?;

    if authenticated_peer_node != PEER_EGRESS_NODE {
        return Err(format!(
            "authenticated peer node {authenticated_peer_node} != expected {PEER_EGRESS_NODE}"
        ));
    }
    if returned_addresses.is_empty() {
        return Err("peer rescue returned no public information".to_owned());
    }

    Ok(CourtEvidence {
        default_information_probe_failed: true,
        selected_peer_path: true,
        authenticated_peer_node,
        returned_addresses,
    })
}

fn default_path_failure_ledger() -> RecoveryLedger {
    let mut ledger = RecoveryLedger::new();

    ledger.register("default:https", ProbeKind::TinyHttps);
    ledger.register("default:tcp", ProbeKind::Tcp);
    ledger.register("peer:local-contact", ProbeKind::LanPeer);

    ledger.set_status(
        "default:https",
        ProbeStatus::Failed,
        Some("simulated default application path failure".to_owned()),
    );
    ledger.set_status(
        "default:tcp",
        ProbeStatus::Failed,
        Some("simulated default TCP path failure".to_owned()),
    );
    ledger.set_status(
        "peer:local-contact",
        ProbeStatus::Succeeded,
        Some("authenticated peer transport candidate available".to_owned()),
    );

    ledger
}

fn recovery_graph() -> ConnectivityGraph {
    let mut graph = ConnectivityGraph::new();

    graph.upsert_node(NodeProfile::local(LOCAL_NODE));
    graph.upsert_node(NodeProfile::egress(DEAD_DEFAULT_EGRESS, false));
    graph.upsert_node(NodeProfile::egress(PEER_EGRESS_NODE, false));

    graph.observe_link(LinkObservation {
        from: LOCAL_NODE,
        to: DEAD_DEFAULT_EGRESS,
        transport: Transport::Wifi,
        reachability: Reachability::Internet,
        state: LinkState::Down,
        estimated_bitrate_bps: 50_000_000,
        loss_ppm: 1_000_000,
        rtt: Duration::from_secs(2),
        energy_cost: 20,
        metered: false,
        last_success_age: Duration::from_secs(600),
    });

    graph.observe_link(LinkObservation {
        from: LOCAL_NODE,
        to: PEER_EGRESS_NODE,
        transport: Transport::WifiDirect,
        reachability: Reachability::PeerOnly,
        state: LinkState::Up,
        estimated_bitrate_bps: 100,
        loss_ppm: 100_000,
        rtt: Duration::from_millis(180),
        energy_cost: 25,
        metered: false,
        last_success_age: Duration::ZERO,
    });

    graph
}

fn resolve_through_authenticated_peer() -> Result<(u64, Vec<IpAddr>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("bind mock peer egress: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("read mock peer egress address: {error}"))?;

    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| format!("accept rescue client: {error}"))?;

        let key = PeerKey::new([0x91; 32]);
        let mut replay = NonceReplayCache::new(32);
        let (client_node, mut session) = perform_server_handshake(
            &mut stream,
            PEER_EGRESS_NODE,
            &key,
            &mut replay,
        )
        .map_err(|error| format!("server secure handshake: {error:?}"))?;

        if client_node != LOCAL_NODE {
            return Err(format!(
                "server authenticated unexpected client node {client_node}"
            ));
        }

        serve_one(&mut session, &mut stream, &FixedResolver)
            .map_err(|error| format!("serve constrained peer egress: {error:?}"))
    });

    let mut stream = TcpStream::connect(address)
        .map_err(|error| format!("connect rescue peer: {error}"))?;
    let key = PeerKey::new([0x91; 32]);
    let (peer_node, mut session) =
        perform_client_handshake(&mut stream, LOCAL_NODE, &key)
            .map_err(|error| format!("client secure handshake: {error:?}"))?;

    let addresses = resolve_via_peer(
        &mut session,
        &mut stream,
        REQUEST_ID,
        HOSTNAME,
    )
    .map_err(|error| format!("resolve via rescue peer: {error:?}"))?;

    server
        .join()
        .map_err(|_| "mock peer egress thread panicked".to_owned())??;

    Ok((peer_node, addresses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectivity_core::DeliveryMode;

    #[test]
    fn court_requires_failed_default_but_keeps_peer_information_path() {
        let ledger = default_path_failure_ledger();

        assert!(ledger.exhaustive_complete());
        assert!(ledger.any_information_path());
        assert!(!ledger.can_declare_local_only());
    }

    #[test]
    fn planner_ignores_dead_default_and_selects_peer_rescue() {
        let graph = recovery_graph();
        let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);

        let RecoveryPlan::Live(plan) =
            plan_recovery(&graph, LOCAL_NODE, &task)
        else {
            panic!("expected live peer rescue plan");
        };

        assert_eq!(plan.path_kind, RecoveryPathKind::PeerEgress);
        assert_eq!(
            plan.route_nodes,
            vec![LOCAL_NODE, PEER_EGRESS_NODE]
        );
        assert_eq!(plan.effective_bps, 90);
        assert_eq!(plan.mode, DeliveryMode::TinySemantic);
        assert!(plan.expected_completion > Duration::from_secs(20));
    }

    #[test]
    fn full_software_recovery_court_returns_peer_information() {
        let evidence = run_software_court().unwrap();

        assert!(evidence.default_information_probe_failed);
        assert!(evidence.selected_peer_path);
        assert_eq!(
            evidence.authenticated_peer_node,
            PEER_EGRESS_NODE
        );
        assert_eq!(
            evidence.returned_addresses,
            vec!["8.8.8.8".parse::<IpAddr>().unwrap()]
        );
    }
}
