use crate::model::{LinkObservation, LinkState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficClass {
    /// SOS, control messages, and other data where delivery matters most.
    Critical,
    /// Small semantic queries where a few bytes/sec can still be useful.
    TinySemantic,
    /// Interactive traffic where delay matters.
    Interactive,
    /// Large transfers; bandwidth dominates.
    Bulk,
}

/// Returns a cost where lower is better. None means unusable.
///
/// The first policy is deliberately simple and inspectable. It is a baseline,
/// not a claim that these weights are optimal.
pub fn score_link(link: &LinkObservation, class: TrafficClass) -> Option<f64> {
    if !link.usable() {
        return None;
    }

    let bitrate = link.estimated_bitrate_bps.max(1) as f64;
    let loss = link.loss_ppm as f64 / 1_000_000.0;
    let rtt_ms = link.rtt.as_secs_f64() * 1000.0;
    let age_s = link.last_success_age.as_secs_f64();

    let state_penalty = match link.state {
        LinkState::Up => 0.0,
        LinkState::Intermittent => match class {
            TrafficClass::Critical | TrafficClass::TinySemantic => 8.0,
            TrafficClass::Interactive => 80.0,
            TrafficClass::Bulk => 150.0,
        },
        LinkState::Down => return None,
    };

    let bandwidth_penalty = match class {
        TrafficClass::Critical => 25.0 / bitrate.sqrt(),
        TrafficClass::TinySemantic => 100.0 / bitrate.sqrt(),
        TrafficClass::Interactive => 500.0 / bitrate.sqrt(),
        TrafficClass::Bulk => 20_000.0 / bitrate.sqrt(),
    };

    let latency_weight = match class {
        TrafficClass::Critical => 0.006,
        TrafficClass::TinySemantic => 0.002,
        TrafficClass::Interactive => 0.02,
        TrafficClass::Bulk => 0.001,
    };

    let loss_weight = match class {
        TrafficClass::Critical => 180.0,
        TrafficClass::TinySemantic => 120.0,
        TrafficClass::Interactive => 220.0,
        TrafficClass::Bulk => 300.0,
    };

    let energy_weight = match class {
        TrafficClass::Critical => 0.05,
        TrafficClass::TinySemantic => 0.10,
        TrafficClass::Interactive => 0.15,
        TrafficClass::Bulk => 0.35,
    };

    let metered_penalty = if link.metered && class == TrafficClass::Bulk {
        30.0
    } else {
        0.0
    };

    Some(
        state_penalty
            + bandwidth_penalty
            + rtt_ms * latency_weight
            + loss * loss_weight
            + f64::from(link.energy_cost) * energy_weight
            + age_s.min(300.0) * 0.02
            + metered_penalty,
    )
}
