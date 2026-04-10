//! Node load balancer for VM placement.
//!
//! This crate implements the algorithm that decides which compute node should
//! host a new VM. It is intentionally free of IO, databases, and async — it
//! operates on plain data structs and returns deterministic (or weighted-random)
//! results.
//!
//! # Design
//!
//! The orchestrator feeds node snapshots (capacity + health + telemetry) into
//! [`select_nodes`], which returns a ranked list of candidates. The caller
//! walks the list, trying each node until provisioning succeeds.
//!
//! Speculative resource tracking ([`PendingAllocations`]) prevents concurrent
//! requests from piling onto the same node between health check cycles.
//!
//! # Algorithm Overview
//!
//! ```text
//! 1. Filter to healthy nodes
//! 2. Filter by resource requirements (if any)
//! 3. Score each node: weighted(cpu_avail%, mem_avail%) minus pending
//! 4. Rank: sticky placement if preferred node is good enough,
//!    otherwise weighted random from top candidates
//! 5. Return ranked list; caller iterates with try-next-on-failure
//! ```

mod pending;
pub mod scoring;
mod selection;

pub use pending::PendingAllocations;
pub use scoring::{ScoringWeights, DEFAULT_WEIGHTS};
pub use selection::{
    select_nodes, NodeCandidate, NodeSnapshot, SelectionConfig, SelectionError, SelectionInput,
    SelectionResult, VmRequirements,
};
