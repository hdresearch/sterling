//! Node selection for VM placement — thin wrapper around [`node_balancer`].
//!
//! This module adapts the orchestrator's DB entities into the pure-data types
//! that [`node_balancer::select_nodes`] expects, and wraps the result in
//! orchestrator-specific types ([`NodeCandidates`], [`ResourceReservation`]).

use std::sync::{Arc, LazyLock, RwLock};

use thiserror::Error;
use uuid::Uuid;

use node_balancer::{
    NodeSnapshot, PendingAllocations, SelectionConfig, SelectionError, SelectionInput,
    VmRequirements as BalancerVmRequirements, select_nodes,
};

use crate::{
    action::{Action, ActionContext},
    db::{ChelseaNodeRepository, DBError, HealthCheckRepository},
};

// ============================================================================
// Global pending allocations (thread-safe wrapper around the crate type)
// ============================================================================

static PENDING_ALLOCATIONS: LazyLock<Arc<RwLock<PendingAllocations>>> =
    LazyLock::new(|| Arc::new(RwLock::new(PendingAllocations::new())));

/// Clear all pending allocations for a node.
///
/// Called when fresh telemetry arrives from a health check, since the telemetry
/// now reflects the actual resource usage on the node (including all committed
/// VMs). Any remaining pending allocations for that node are stale at that point.
pub fn clear_pending_for_node(node_id: &Uuid) {
    let mut pending = PENDING_ALLOCATIONS.write().unwrap();
    pending.clear_node(node_id);
}

fn add_pending(node_id: Uuid, vcpu: u32, mem_mib: u32) {
    let mut pending = PENDING_ALLOCATIONS.write().unwrap();
    pending.add(node_id, vcpu, mem_mib);
}

fn remove_pending(node_id: Uuid, vcpu: u32, mem_mib: u32) {
    let mut pending = PENDING_ALLOCATIONS.write().unwrap();
    pending.remove(node_id, vcpu, mem_mib);
}

// ============================================================================
// Public types re-exported from this module
// ============================================================================

/// Resource requirements for the VM being placed.
///
/// Used to filter out nodes that don't have enough available resources.
#[derive(Debug, Clone, Copy, Default)]
pub struct VmRequirements {
    /// Number of vCPUs needed (defaults to 1 if not specified)
    pub vcpu_count: u32,
    /// Memory needed in MiB (defaults to 512 if not specified)
    pub mem_size_mib: u32,
}

impl VmRequirements {
    /// Create requirements with explicit values
    pub fn new(vcpu_count: u32, mem_size_mib: u32) -> Self {
        Self {
            vcpu_count,
            mem_size_mib,
        }
    }

    /// Create requirements from optional values, using defaults if not specified
    pub fn from_optional(vcpu_count: Option<u32>, mem_size_mib: Option<u32>) -> Self {
        Self {
            vcpu_count: vcpu_count.unwrap_or(1),
            mem_size_mib: mem_size_mib.unwrap_or(512),
        }
    }
}

impl From<&VmRequirements> for BalancerVmRequirements {
    fn from(r: &VmRequirements) -> Self {
        BalancerVmRequirements::new(r.vcpu_count, r.mem_size_mib)
    }
}

// ============================================================================
// ResourceReservation
// ============================================================================

/// A reservation of resources on a node, returned by ChooseNode.
///
/// This acts as a guard that automatically releases the reserved resources
/// when dropped, unless `commit()` is called to indicate successful placement.
#[must_use = "ResourceReservation must be committed or it will release resources on drop"]
pub struct ResourceReservation {
    node_id: Uuid,
    vcpu: u32,
    mem_mib: u32,
    committed: bool,
}

impl ResourceReservation {
    fn new(node_id: Uuid, requirements: Option<&VmRequirements>) -> Self {
        let (vcpu, mem_mib) = requirements
            .map(|r| (r.vcpu_count, r.mem_size_mib))
            .unwrap_or((0, 0));
        Self {
            node_id,
            vcpu,
            mem_mib,
            committed: false,
        }
    }

    /// Get the ID of the reserved node.
    pub fn node_id(&self) -> Uuid {
        self.node_id
    }

    /// Commit the reservation, indicating the VM was successfully placed.
    ///
    /// The pending allocation is intentionally **kept in place** after commit.
    /// The pending is cleared when the next health check writes fresh telemetry
    /// for this node (via `clear_pending_for_node`).
    pub fn commit(mut self) {
        self.committed = true;
        tracing::debug!(
            node_id = %self.node_id,
            vcpu = self.vcpu,
            mem_mib = self.mem_mib,
            "Resource reservation committed (pending kept until next health check)"
        );
    }
}

impl Drop for ResourceReservation {
    fn drop(&mut self) {
        if !self.committed {
            remove_pending(self.node_id, self.vcpu, self.mem_mib);
            tracing::debug!(
                node_id = %self.node_id,
                vcpu = self.vcpu,
                mem_mib = self.mem_mib,
                "Resource reservation released (not committed)"
            );
        }
    }
}

// ============================================================================
// NodeCandidates
// ============================================================================

/// A ranked list of candidate nodes for VM placement.
///
/// Returned by `ChooseNode`. The caller walks the list with `next_node()`,
/// trying each node until provisioning succeeds.
pub struct NodeCandidates {
    ranked: Vec<Uuid>,
    index: usize,
    requirements: Option<VmRequirements>,
}

impl NodeCandidates {
    /// Get the next candidate node with a resource reservation.
    ///
    /// Returns `None` when all candidates have been exhausted.
    pub fn next_node(&mut self) -> Option<ResourceReservation> {
        let &node_id = self.ranked.get(self.index)?;
        self.index += 1;

        if let Some(ref reqs) = self.requirements {
            add_pending(node_id, reqs.vcpu_count, reqs.mem_size_mib);
        }

        Some(ResourceReservation::new(
            node_id,
            self.requirements.as_ref(),
        ))
    }

    /// Get the candidate at a specific index with a resource reservation.
    ///
    /// Unlike `next_node()`, this does not advance the internal cursor.
    pub fn get_node(&self, index: usize) -> Option<ResourceReservation> {
        let &node_id = self.ranked.get(index)?;

        if let Some(ref reqs) = self.requirements {
            add_pending(node_id, reqs.vcpu_count, reqs.mem_size_mib);
        }

        Some(ResourceReservation::new(
            node_id,
            self.requirements.as_ref(),
        ))
    }

    /// How many candidates remain (haven't been tried yet).
    pub fn remaining(&self) -> usize {
        self.ranked.len().saturating_sub(self.index)
    }
}

// ============================================================================
// ChooseNode Action
// ============================================================================

/// Node selector that chooses the best compute nodes for VM placement.
///
/// Returns a `NodeCandidates` list ranked by suitability. The caller
/// walks the list with `next_node()`, trying each node in turn until
/// provisioning succeeds.
pub struct ChooseNode {
    preferred_node_id: Option<Uuid>,
    requirements: Option<VmRequirements>,
}

impl ChooseNode {
    pub fn new() -> Self {
        Self {
            preferred_node_id: None,
            requirements: None,
        }
    }

    /// Create a node selector with a preferred node for sticky placement.
    pub fn with_preferred_node(node_id: Uuid) -> Self {
        Self {
            preferred_node_id: Some(node_id),
            requirements: None,
        }
    }

    /// Add resource requirements for the VM being placed.
    pub fn with_requirements(mut self, requirements: VmRequirements) -> Self {
        self.requirements = Some(requirements);
        self
    }
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Error)]
pub enum RechooseNodeError {
    #[error("no healthy nodes available")]
    NoNodes,

    #[error(
        "no nodes with sufficient resources (need {vcpu_required} vCPUs, {mem_required_mib} MiB RAM)"
    )]
    InsufficientResources {
        vcpu_required: u32,
        mem_required_mib: u32,
    },

    #[error("internal error")]
    InternalError,

    #[error("database error: {0}")]
    DB(#[from] DBError),
}

impl From<SelectionError> for RechooseNodeError {
    fn from(err: SelectionError) -> Self {
        match err {
            SelectionError::NoNodes => RechooseNodeError::NoNodes,
            SelectionError::InsufficientResources {
                vcpu_required,
                mem_required_mib,
            } => RechooseNodeError::InsufficientResources {
                vcpu_required,
                mem_required_mib,
            },
        }
    }
}

// ============================================================================
// Action implementation
// ============================================================================

impl Action for ChooseNode {
    type Response = NodeCandidates;
    type Error = RechooseNodeError;

    const ACTION_ID: &'static str = "nodes.choose";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        // Prune stale pending allocations
        {
            let mut pending = PENDING_ALLOCATIONS.write().unwrap();
            pending.prune_stale();
        }

        let nodes = ctx.db.node().all_under_orchestrator(ctx.orch.id()).await?;

        tracing::info!(
            total_registered = nodes.len(),
            orchestrator_id = %ctx.orch.id(),
            requirements = ?self.requirements,
            preferred_node = ?self.preferred_node_id,
            "Node selection starting"
        );

        if nodes.is_empty() {
            tracing::warn!(orchestrator_id = %ctx.orch.id(), "No nodes registered");
            return Err(RechooseNodeError::NoNodes);
        }

        // Build NodeSnapshots from DB entities + health checks
        let mut snapshots = Vec::with_capacity(nodes.len());

        for node in &nodes {
            let health_checks = ctx.db.health().last_5(node.id()).await?;
            let latest = health_checks.first();

            let healthy = latest.is_some_and(|hc| hc.status().is_up());

            let (available_vcpu, available_mem_mib) = latest
                .map(|hc| (hc.vcpu_available(), hc.mem_mib_available()))
                .unwrap_or((None, None));

            snapshots.push(NodeSnapshot {
                id: *node.id(),
                healthy,
                total_cpu: node.resources().hardware_cpu(),
                total_mem_mib: node.resources().hardware_memory_mib(),
                available_vcpu,
                available_mem_mib,
            });
        }

        // Call the node_balancer algorithm
        let balancer_reqs = self.requirements.as_ref().map(BalancerVmRequirements::from);
        let config = SelectionConfig::default();

        let result = {
            let pending = PENDING_ALLOCATIONS.read().unwrap();
            let input = SelectionInput {
                nodes: &snapshots,
                preferred_node_id: self.preferred_node_id,
                requirements: balancer_reqs,
                pending: &pending,
                config: &config,
            };
            select_nodes(&input)?
        };

        tracing::info!(
            healthy_count = result.healthy_count,
            skipped_resources = result.skipped_resources,
            skipped_no_telemetry = result.skipped_no_telemetry,
            candidate_count = result.ranked.len(),
            preferred_node = ?self.preferred_node_id,
            "Node selection complete"
        );

        for (rank, candidate) in result.ranked.iter().enumerate() {
            tracing::debug!(
                rank = rank + 1,
                node_id = %candidate.id,
                score = format!("{:.2}", candidate.score),
                "Candidate"
            );
        }

        Ok(NodeCandidates {
            ranked: result.node_ids(),
            index: 0,
            requirements: self.requirements,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_add_and_clear() {
        let id = Uuid::new_v4();
        add_pending(id, 4, 1024);
        {
            let pending = PENDING_ALLOCATIONS.read().unwrap();
            assert_eq!(pending.get(&id), (4, 1024));
        }
        clear_pending_for_node(&id);
        {
            let pending = PENDING_ALLOCATIONS.read().unwrap();
            assert_eq!(pending.get(&id), (0, 0));
        }
    }

    #[test]
    fn pending_remove_releases() {
        let id = Uuid::new_v4();
        add_pending(id, 4, 1024);
        remove_pending(id, 2, 512);
        let pending = PENDING_ALLOCATIONS.read().unwrap();
        assert_eq!(pending.get(&id), (2, 512));
    }

    #[test]
    fn vm_requirements_defaults() {
        let reqs = VmRequirements::from_optional(None, None);
        assert_eq!(reqs.vcpu_count, 1);
        assert_eq!(reqs.mem_size_mib, 512);
    }

    #[test]
    fn vm_requirements_explicit() {
        let reqs = VmRequirements::new(8, 16384);
        assert_eq!(reqs.vcpu_count, 8);
        assert_eq!(reqs.mem_size_mib, 16384);
    }

    #[test]
    fn selection_error_converts_to_rechoose_error() {
        let err: RechooseNodeError = SelectionError::NoNodes.into();
        assert!(matches!(err, RechooseNodeError::NoNodes));

        let err: RechooseNodeError = SelectionError::InsufficientResources {
            vcpu_required: 4,
            mem_required_mib: 8192,
        }
        .into();
        assert!(matches!(
            err,
            RechooseNodeError::InsufficientResources {
                vcpu_required: 4,
                mem_required_mib: 8192
            }
        ));
    }
}
