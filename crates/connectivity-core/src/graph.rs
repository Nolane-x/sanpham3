use crate::model::{LinkObservation, NodeId, NodeProfile, Reachability};
use crate::scoring::{score_link, TrafficClass};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Route {
    pub nodes: Vec<NodeId>,
    pub total_cost: f64,
    pub bottleneck_bps: u64,
    pub worst_loss_ppm: u32,
    pub total_rtt: Duration,
    pub intermittent_hops: u16,
    pub metered_hops: u16,
    pub peer_only_hops: u16,
}

impl Route {
    pub fn hop_count(&self) -> usize {
        self.nodes.len().saturating_sub(1)
    }

    /// Conservative goodput estimate used by recovery planning.
    ///
    /// It discounts the bottleneck by the worst observed loss and applies an
    /// additional 50% penalty per intermittent hop. It is intentionally
    /// conservative and remains an estimate, not a physical-layer guarantee.
    pub fn estimated_effective_bps(&self) -> u64 {
        if self.bottleneck_bps == u64::MAX {
            return u64::MAX;
        }

        let delivered_ppm =
            1_000_000_u64.saturating_sub(u64::from(self.worst_loss_ppm));
        let mut effective = (u128::from(self.bottleneck_bps)
            * u128::from(delivered_ppm)
            / 1_000_000_u128)
            .min(u128::from(u64::MAX)) as u64;

        for _ in 0..self.intermittent_hops {
            effective /= 2;
        }

        effective.max(1)
    }
}

#[derive(Default)]
pub struct ConnectivityGraph {
    nodes: HashMap<NodeId, NodeProfile>,
    links: Vec<LinkObservation>,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    cost: f64,
    node: NodeId,
}

#[derive(Clone, Copy, Debug)]
struct PreviousHop {
    parent: NodeId,
    bitrate_bps: u64,
    loss_ppm: u32,
    rtt: Duration,
    intermittent: bool,
    metered: bool,
    peer_only: bool,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node && self.cost.to_bits() == other.cost.to_bits()
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.node.cmp(&other.node))
    }
}

impl ConnectivityGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert_node(&mut self, node: NodeProfile) {
        self.nodes.insert(node.id, node);
    }

    pub fn observe_link(&mut self, link: LinkObservation) {
        if let Some(existing) = self.links.iter_mut().find(|candidate| {
            candidate.from == link.from
                && candidate.to == link.to
                && candidate.transport == link.transport
        }) {
            *existing = link;
        } else {
            self.links.push(link);
        }
    }

    pub fn nodes(&self) -> impl Iterator<Item = &NodeProfile> {
        self.nodes.values()
    }

    pub fn links(&self) -> impl Iterator<Item = &LinkObservation> {
        self.links.iter()
    }

    /// Finds the cheapest reachable node currently advertising Internet egress.
    pub fn best_egress_route(
        &self,
        start: NodeId,
        class: TrafficClass,
    ) -> Option<Route> {
        if self.nodes.get(&start)?.internet_egress {
            return Some(Route {
                nodes: vec![start],
                total_cost: 0.0,
                bottleneck_bps: u64::MAX,
                worst_loss_ppm: 0,
                total_rtt: Duration::ZERO,
                intermittent_hops: 0,
                metered_hops: 0,
                peer_only_hops: 0,
            });
        }

        let mut dist: HashMap<NodeId, f64> = HashMap::new();
        let mut prev: HashMap<NodeId, PreviousHop> = HashMap::new();
        let mut heap = BinaryHeap::new();

        dist.insert(start, 0.0);
        heap.push(Candidate {
            cost: 0.0,
            node: start,
        });

        let mut goal = None;

        while let Some(Candidate { cost, node }) = heap.pop() {
            if cost > *dist.get(&node).unwrap_or(&f64::INFINITY) {
                continue;
            }

            if node != start
                && self
                    .nodes
                    .get(&node)
                    .is_some_and(|candidate| candidate.internet_egress)
            {
                goal = Some(node);
                break;
            }

            let relay_allowed = node == start
                || self
                    .nodes
                    .get(&node)
                    .map(|candidate| candidate.relay_allowed)
                    .unwrap_or(false);

            if !relay_allowed {
                continue;
            }

            for link in self.links.iter().filter(|candidate| candidate.from == node) {
                let Some(link_cost) = score_link(link, class) else {
                    continue;
                };

                if !self.nodes.contains_key(&link.to) {
                    continue;
                }

                // Small hop penalty: equal-quality paths should prefer fewer relays.
                let next = cost + link_cost + 2.0;
                if next < *dist.get(&link.to).unwrap_or(&f64::INFINITY) {
                    dist.insert(link.to, next);
                    prev.insert(
                        link.to,
                        PreviousHop {
                            parent: node,
                            bitrate_bps: link.estimated_bitrate_bps,
                            loss_ppm: link.loss_ppm,
                            rtt: link.rtt,
                            intermittent:
                                link.state == crate::model::LinkState::Intermittent,
                            metered: link.metered,
                            peer_only:
                                link.reachability == Reachability::PeerOnly,
                        },
                    );
                    heap.push(Candidate {
                        cost: next,
                        node: link.to,
                    });
                }
            }
        }

        let goal = goal?;
        let mut cursor = goal;
        let mut nodes = vec![goal];
        let mut bottleneck = u64::MAX;
        let mut worst_loss_ppm = 0_u32;
        let mut total_rtt = Duration::ZERO;
        let mut intermittent_hops = 0_u16;
        let mut metered_hops = 0_u16;
        let mut peer_only_hops = 0_u16;

        while cursor != start {
            let hop = *prev.get(&cursor)?;
            bottleneck = bottleneck.min(hop.bitrate_bps);
            worst_loss_ppm = worst_loss_ppm.max(hop.loss_ppm);
            total_rtt = total_rtt.saturating_add(hop.rtt);
            intermittent_hops = intermittent_hops
                .saturating_add(u16::from(hop.intermittent));
            metered_hops =
                metered_hops.saturating_add(u16::from(hop.metered));
            peer_only_hops =
                peer_only_hops.saturating_add(u16::from(hop.peer_only));
            cursor = hop.parent;
            nodes.push(cursor);
        }

        nodes.reverse();

        Some(Route {
            nodes,
            total_cost: *dist.get(&goal)?,
            bottleneck_bps: bottleneck,
            worst_loss_ppm,
            total_rtt,
            intermittent_hops,
            metered_hops,
            peer_only_hops,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LinkState, Reachability, Transport};
    use std::time::Duration;

    fn link(
        from: NodeId,
        to: NodeId,
        bitrate_bps: u64,
        loss_ppm: u32,
    ) -> LinkObservation {
        LinkObservation {
            from,
            to,
            transport: Transport::WifiDirect,
            reachability: Reachability::PeerOnly,
            state: LinkState::Up,
            estimated_bitrate_bps: bitrate_bps,
            loss_ppm,
            rtt: Duration::from_millis(20),
            energy_cost: 10,
            metered: false,
            last_success_age: Duration::ZERO,
        }
    }

    #[test]
    fn finds_multihop_egress() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::local(2));
        graph.upsert_node(NodeProfile::egress(3, false));

        graph.observe_link(link(1, 2, 100_000, 10_000));
        graph.observe_link(link(2, 3, 80, 400_000));

        let route = graph
            .best_egress_route(1, TrafficClass::TinySemantic)
            .expect("route");

        assert_eq!(route.nodes, vec![1, 2, 3]);
        assert_eq!(route.bottleneck_bps, 80);
    }

    #[test]
    fn route_carries_quality_evidence() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::local(2));
        graph.upsert_node(NodeProfile::egress(3, false));

        let mut first = link(1, 2, 1_000, 100_000);
        first.rtt = Duration::from_millis(40);

        let mut second = link(2, 3, 100, 400_000);
        second.rtt = Duration::from_millis(300);
        second.state = LinkState::Intermittent;
        second.metered = true;

        graph.observe_link(first);
        graph.observe_link(second);

        let route = graph
            .best_egress_route(1, TrafficClass::TinySemantic)
            .expect("route");

        assert_eq!(route.hop_count(), 2);
        assert_eq!(route.bottleneck_bps, 100);
        assert_eq!(route.worst_loss_ppm, 400_000);
        assert_eq!(route.total_rtt, Duration::from_millis(340));
        assert_eq!(route.intermittent_hops, 1);
        assert_eq!(route.metered_hops, 1);
        assert_eq!(route.peer_only_hops, 2);
        assert_eq!(route.estimated_effective_bps(), 30);
    }

    #[test]
    fn ignores_dead_links() {
        let mut graph = ConnectivityGraph::new();
        graph.upsert_node(NodeProfile::local(1));
        graph.upsert_node(NodeProfile::egress(2, false));

        let mut dead = link(1, 2, 1000, 0);
        dead.state = LinkState::Down;
        graph.observe_link(dead);

        assert!(graph
            .best_egress_route(1, TrafficClass::TinySemantic)
            .is_none());
    }
}
