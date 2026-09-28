use connectivity_core::{
    Bundle, BundlePriority, CapsuleKind, ConnectivityGraph, DtnQueue,
    plan_recovery, LinkObservation, LinkState, NodeProfile, Reachability,
    RecoveryPlan, RecoveryTask, SemanticCapsule, TrafficClass, Transport,
};
use std::time::{Duration, Instant};

fn main() {
    println!("sanpham3 connectivity recovery demo v0.1");
    println!("normal Internet: unavailable");
    println!("scanning local capability graph...\n");

    let mut graph = ConnectivityGraph::new();

    graph.upsert_node(NodeProfile::local(1));
    graph.upsert_node(NodeProfile::local(2));
    graph.upsert_node(NodeProfile::egress(3, true));

    graph.observe_link(peer_link(
        1,
        2,
        Transport::WifiDirect,
        2_000_000,
        8_000,
        18,
    ));

    graph.observe_link(peer_link(
        2,
        3,
        Transport::BluetoothLe,
        82,
        420_000,
        1200,
    ));

    let route = graph
        .best_egress_route(1, TrafficClass::TinySemantic)
        .expect("demo route must exist");

    println!("route discovered: {:?}", route.nodes);
    println!("bottleneck: {} bit/s", route.bottleneck_bps);
    println!("effective: {} bit/s", route.estimated_effective_bps());
    println!("worst loss: {} ppm", route.worst_loss_ppm);
    println!("route RTT: {} ms", route.total_rtt.as_millis());
    println!("path cost: {:.2}", route.total_cost);

    let task = RecoveryTask::new(TrafficClass::TinySemantic, 232);
    match plan_recovery(&graph, 1, &task) {
        RecoveryPlan::Live(plan) => {
            println!(
                "adaptive plan: {:?} over {:?}, expected {} ms{}",
                plan.mode,
                plan.path_kind,
                plan.expected_completion.as_millis(),
                if plan.experimental {
                    " (experimental)"
                } else {
                    ""
                },
            );
        }
        RecoveryPlan::DelayTolerant { reason, .. } => {
            println!("adaptive plan: DTN queue ({reason:?})");
        }
        RecoveryPlan::LocalOnly { reason } => {
            println!("adaptive plan: local only ({reason:?})");
        }
    }
    println!();

    let query = SemanticCapsule {
        kind: CapsuleKind::Query,
        flags: 0,
        request_id: 7,
        ttl_hops: 8,
        payload: b"weather:haiphong:now".to_vec(),
    };

    let wire = query.encode().expect("encode");
    println!(
        "semantic query: {} payload bytes, {} bytes on wire",
        query.payload.len(),
        wire.len()
    );

    let now = Instant::now();
    let mut dtn = DtnQueue::new();

    dtn.push(Bundle {
        id: 7,
        priority: BundlePriority::Urgent,
        created_at: now,
        ttl: Duration::from_secs(3600),
        payload: wire,
        attempts: 0,
    });

    let next = dtn.next_for_send(now).unwrap();
    println!("DTN bundle {} ready, attempt {}", next.id, next.attempts);
    println!("\nUSEFUL INTERNET PATH RECOVERED");
}

fn peer_link(
    from: u64,
    to: u64,
    transport: Transport,
    bitrate: u64,
    loss_ppm: u32,
    rtt_ms: u64,
) -> LinkObservation {
    LinkObservation {
        from,
        to,
        transport,
        reachability: Reachability::PeerOnly,
        state: if loss_ppm > 300_000 {
            LinkState::Intermittent
        } else {
            LinkState::Up
        },
        estimated_bitrate_bps: bitrate,
        loss_ppm,
        rtt: Duration::from_millis(rtt_ms),
        energy_cost: 20,
        metered: false,
        last_success_age: Duration::from_secs(1),
    }
}
