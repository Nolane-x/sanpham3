use crate::model::{LinkObservation, NodeId, NodeProfile};
use crate::scoring::{score_link, TrafficClass};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

#[derive(Debug, Clone)]
pub struct Route {
    pub nodes: Vec<NodeId>,
    pub total_cost: f64,
    pub bottleneck_bps: u64,
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
            });
        }

        let mut dist: HashMap<NodeId, f64> = HashMap::new();
        let mut prev: HashMap<NodeId, (NodeId, u64)> = HashMap::new();
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
                    prev.insert(link.to, (node, link.estimated_bitrate_bps));
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

        while cursor != start {
            let (parent, bitrate) = *prev.get(&cursor)?;
            bottleneck = bottleneck.min(bitrate);
            cursor = parent;
            nodes.push(cursor);
        }

        nodes.reverse();

        Some(Route {
            nodes,
            total_cost: *dist.get(&goal)?,
            bottleneck_bps: bottleneck,
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
