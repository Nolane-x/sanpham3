//! OS-agnostic primitives for app-only extreme connectivity recovery.
//!
//! Android, Windows, and Linux adapters feed measured capabilities and link
//! observations into this crate. The core scores links, builds peer/egress
//! routes, queues delay-tolerant work, and carries compact semantic capsules.

pub mod capsule;
pub mod dtn;
pub mod graph;
pub mod model;
pub mod probe;
pub mod recovery;
pub mod scoring;

pub use capsule::{CapsuleError, CapsuleKind, SemanticCapsule};
pub use dtn::{Bundle, BundlePriority, DtnQueue};
pub use graph::{ConnectivityGraph, Route};
pub use model::{
    LinkObservation, LinkState, NodeId, NodeProfile, Reachability, Transport,
};
pub use probe::{Capability, PermissionState, PlatformScanner};
pub use recovery::{ProbeKind, ProbeRecord, ProbeStatus, RecoveryLedger};
pub use scoring::{score_link, TrafficClass};
