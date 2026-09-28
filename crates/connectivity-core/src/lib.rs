//! OS-agnostic primitives for app-only extreme connectivity recovery.
//!
//! Android, Windows, and Linux adapters feed measured capabilities and link
//! observations into this crate. The core scores links, builds peer/egress
//! routes, queues delay-tolerant work, and carries compact semantic capsules.

pub mod capsule;
pub mod dtn;
pub mod graph;
pub mod model;
pub mod policy;
pub mod probe;
pub mod recovery;
pub mod scoring;

pub use capsule::{CapsuleError, CapsuleKind, SemanticCapsule};
pub use dtn::{Bundle, BundlePriority, DtnQueue};
pub use graph::{ConnectivityGraph, Route};
pub use model::{
    conservative_transport_defaults, LinkObservation, LinkState,
    MeasuredPathEvidence, NodeId, NodeProfile, Reachability, Transport,
    TransportSchedulingDefaults,
};
pub use policy::{
    plan_recovery, DeliveryMode, LiveRecoveryPlan, PlanReason, RecoveryPathKind,
    RecoveryPlan, RecoveryTask,
};
pub use probe::{Capability, PermissionState, PlatformScanner};
pub use recovery::{ProbeKind, ProbeRecord, ProbeStatus, RecoveryLedger};
pub use scoring::{score_link, TrafficClass};
