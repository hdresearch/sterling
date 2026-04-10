//! Speculative resource tracking for in-flight VM placements.
//!
//! Between health check cycles (~5s), multiple concurrent requests may try to
//! place VMs. Without tracking, they all see identical available resources and
//! pile onto the same "best" node.
//!
//! [`PendingAllocations`] tracks speculative reservations: when a node is
//! selected, we debit its resources here. The reservation is released on
//! failure (drop) or kept until the next health check on success (commit).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// How long before a pending allocation is considered stale and pruned.
const DEFAULT_TTL: Duration = Duration::from_secs(120);

/// Tracks in-flight resource reservations per node.
///
/// Thread-safety: the caller is responsible for synchronization (e.g. wrapping
/// in `Arc<Mutex<_>>` or `Arc<RwLock<_>>`). This keeps the crate sync-primitive
/// agnostic.
#[derive(Debug)]
pub struct PendingAllocations {
    entries: HashMap<Uuid, PendingEntry>,
    ttl: Duration,
}

#[derive(Debug, Clone)]
struct PendingEntry {
    vcpu: u32,
    mem_mib: u64,
    updated_at: Instant,
}

impl PendingAllocations {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            ttl: DEFAULT_TTL,
        }
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    /// Add resources to a node's pending allocation.
    pub fn add(&mut self, node_id: Uuid, vcpu: u32, mem_mib: u32) {
        if vcpu == 0 && mem_mib == 0 {
            return;
        }
        let now = Instant::now();
        let entry = self.entries.entry(node_id).or_insert(PendingEntry {
            vcpu: 0,
            mem_mib: 0,
            updated_at: now,
        });
        entry.vcpu += vcpu;
        entry.mem_mib += mem_mib as u64;
        entry.updated_at = now;
    }

    /// Remove resources from a node's pending allocation (e.g. on placement failure).
    pub fn remove(&mut self, node_id: Uuid, vcpu: u32, mem_mib: u32) {
        if vcpu == 0 && mem_mib == 0 {
            return;
        }
        if let Some(entry) = self.entries.get_mut(&node_id) {
            entry.vcpu = entry.vcpu.saturating_sub(vcpu);
            entry.mem_mib = entry.mem_mib.saturating_sub(mem_mib as u64);
            if entry.vcpu == 0 && entry.mem_mib == 0 {
                self.entries.remove(&node_id);
            }
        }
    }

    /// Clear all pending allocations for a node.
    ///
    /// Called when fresh telemetry arrives, since the telemetry now reflects
    /// actual resource usage including all committed VMs.
    pub fn clear_node(&mut self, node_id: &Uuid) {
        if let Some(entry) = self.entries.remove(node_id) {
            if entry.vcpu > 0 || entry.mem_mib > 0 {
                tracing::debug!(
                    node_id = %node_id,
                    vcpu = entry.vcpu,
                    mem_mib = entry.mem_mib,
                    "Cleared pending allocations after fresh telemetry"
                );
            }
        }
    }

    /// Get the current pending allocation for a node.
    pub fn get(&self, node_id: &Uuid) -> (u32, u64) {
        self.entries
            .get(node_id)
            .map(|e| (e.vcpu, e.mem_mib))
            .unwrap_or((0, 0))
    }

    /// Prune stale pending allocations older than the TTL.
    pub fn prune_stale(&mut self) {
        let now = Instant::now();
        let before = self.entries.len();

        self.entries.retain(|node_id, entry| {
            let age = now.duration_since(entry.updated_at);
            if age > self.ttl {
                tracing::warn!(
                    node_id = %node_id,
                    vcpu = entry.vcpu,
                    mem_mib = entry.mem_mib,
                    age_secs = age.as_secs(),
                    "Pruning stale pending allocation"
                );
                false
            } else {
                true
            }
        });

        let pruned = before - self.entries.len();
        if pruned > 0 {
            tracing::info!(pruned_count = pruned, "Pruned stale pending allocations");
        }
    }
}

impl Default for PendingAllocations {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_get() {
        let mut pa = PendingAllocations::new();
        let id = Uuid::new_v4();
        pa.add(id, 4, 1024);
        assert_eq!(pa.get(&id), (4, 1024));
    }

    #[test]
    fn add_accumulates() {
        let mut pa = PendingAllocations::new();
        let id = Uuid::new_v4();
        pa.add(id, 2, 512);
        pa.add(id, 2, 512);
        assert_eq!(pa.get(&id), (4, 1024));
    }

    #[test]
    fn remove_releases() {
        let mut pa = PendingAllocations::new();
        let id = Uuid::new_v4();
        pa.add(id, 4, 1024);
        pa.remove(id, 2, 512);
        assert_eq!(pa.get(&id), (2, 512));
    }

    #[test]
    fn remove_saturates_at_zero() {
        let mut pa = PendingAllocations::new();
        let id = Uuid::new_v4();
        pa.add(id, 2, 512);
        pa.remove(id, 10, 2048);
        assert_eq!(pa.get(&id), (0, 0));
    }

    #[test]
    fn clear_node_removes_entry() {
        let mut pa = PendingAllocations::new();
        let id = Uuid::new_v4();
        pa.add(id, 4, 1024);
        pa.clear_node(&id);
        assert_eq!(pa.get(&id), (0, 0));
    }

    #[test]
    fn get_unknown_node_returns_zero() {
        let pa = PendingAllocations::new();
        assert_eq!(pa.get(&Uuid::new_v4()), (0, 0));
    }

    #[test]
    fn zero_add_is_noop() {
        let mut pa = PendingAllocations::new();
        let id = Uuid::new_v4();
        pa.add(id, 0, 0);
        assert_eq!(pa.get(&id), (0, 0));
    }

    #[test]
    fn prune_stale_removes_old_entries() {
        let mut pa = PendingAllocations::with_ttl(Duration::from_millis(1));
        let id = Uuid::new_v4();
        pa.add(id, 4, 1024);
        std::thread::sleep(Duration::from_millis(5));
        pa.prune_stale();
        assert_eq!(pa.get(&id), (0, 0));
    }

    #[test]
    fn prune_stale_keeps_fresh_entries() {
        let mut pa = PendingAllocations::with_ttl(Duration::from_secs(60));
        let id = Uuid::new_v4();
        pa.add(id, 4, 1024);
        pa.prune_stale();
        assert_eq!(pa.get(&id), (4, 1024));
    }
}
