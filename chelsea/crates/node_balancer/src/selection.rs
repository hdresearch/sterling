//! Node selection: filtering, ranking, and weighted-random picking.
//!
//! The entry point is [`select_nodes`], which takes a list of node snapshots
//! and returns a ranked list of candidates.

use rand::Rng;
use uuid::Uuid;

use crate::pending::PendingAllocations;
use crate::scoring::{compute_score, AvailableResources, NodeCapacity, ScoringWeights};

// ============================================================================
// Configuration
// ============================================================================

/// Tunable knobs for the selection algorithm.
#[derive(Debug, Clone)]
pub struct SelectionConfig {
    /// Minimum score threshold for sticky placement (fraction of max score).
    /// If the preferred node scores below this, we fall back to load balancing.
    /// Default: 0.2 (20% of max)
    pub sticky_score_threshold: f64,

    /// Minimum score threshold for candidate selection (fraction of max score).
    /// Only nodes above this are considered for weighted random. Default: 0.5
    pub candidate_score_threshold: f64,

    /// Minimum candidates to ensure load distribution. Default: 2
    pub min_candidates: usize,

    /// Maximum candidates to bound complexity. Default: 10
    pub max_candidates: usize,

    /// Scoring weights for CPU vs memory.
    pub weights: ScoringWeights,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            sticky_score_threshold: 0.4,
            candidate_score_threshold: 0.5,
            min_candidates: 2,
            max_candidates: 10,
            weights: crate::DEFAULT_WEIGHTS,
        }
    }
}

// ============================================================================
// Input types
// ============================================================================

/// A point-in-time snapshot of a node's state, fed by the orchestrator.
///
/// The orchestrator maps its DB entities into this struct before calling
/// [`select_nodes`]. This keeps the balancer free of DB dependencies.
#[derive(Debug, Clone)]
pub struct NodeSnapshot {
    pub id: Uuid,
    /// Is this node healthy (responding to health checks)?
    pub healthy: bool,
    /// Total hardware capacity.
    pub total_cpu: i32,
    pub total_mem_mib: i64,
    /// Available resources from the most recent telemetry.
    /// `None` if no telemetry has been received yet.
    pub available_vcpu: Option<i32>,
    pub available_mem_mib: Option<i64>,
}

/// Resource requirements for the VM being placed.
#[derive(Debug, Clone, Default)]
pub struct VmRequirements {
    /// Number of vCPUs needed (defaults to 1)
    pub vcpu_count: u32,
    /// Memory needed in MiB (defaults to 512)
    pub mem_size_mib: u32,
}

impl VmRequirements {
    pub fn new(vcpu_count: u32, mem_size_mib: u32) -> Self {
        Self {
            vcpu_count,
            mem_size_mib,
        }
    }

    pub fn from_optional(vcpu_count: Option<u32>, mem_size_mib: Option<u32>) -> Self {
        Self {
            vcpu_count: vcpu_count.unwrap_or(1),
            mem_size_mib: mem_size_mib.unwrap_or(512),
        }
    }
}

/// All inputs for a single selection decision.
pub struct SelectionInput<'a> {
    pub nodes: &'a [NodeSnapshot],
    pub preferred_node_id: Option<Uuid>,
    pub requirements: Option<VmRequirements>,
    pub pending: &'a PendingAllocations,
    pub config: &'a SelectionConfig,
}

// ============================================================================
// Output types
// ============================================================================

/// A candidate node with its computed score.
#[derive(Debug, Clone)]
pub struct NodeCandidate {
    pub id: Uuid,
    pub score: f64,
}

/// The result of node selection: a ranked list of candidates.
#[derive(Debug)]
pub struct SelectionResult {
    /// Node IDs ranked by preference (best first).
    pub ranked: Vec<NodeCandidate>,
    /// How many nodes were healthy.
    pub healthy_count: usize,
    /// How many nodes were skipped for insufficient resources.
    pub skipped_resources: usize,
    /// How many nodes were skipped for missing telemetry.
    pub skipped_no_telemetry: usize,
}

impl SelectionResult {
    /// Get the ranked node IDs (best first).
    pub fn node_ids(&self) -> Vec<Uuid> {
        self.ranked.iter().map(|c| c.id).collect()
    }

    /// Is the result empty (no viable candidates)?
    pub fn is_empty(&self) -> bool {
        self.ranked.is_empty()
    }
}

/// Errors from node selection.
#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("no healthy nodes available")]
    NoNodes,

    #[error(
        "no nodes with sufficient resources (need {vcpu_required} vCPUs, {mem_required_mib} MiB RAM)"
    )]
    InsufficientResources {
        vcpu_required: u32,
        mem_required_mib: u32,
    },
}

// ============================================================================
// Core algorithm
// ============================================================================

/// Select and rank nodes for VM placement.
///
/// Returns a ranked list of candidates, best first. The caller iterates the
/// list, trying each node until provisioning succeeds.
pub fn select_nodes(input: &SelectionInput) -> Result<SelectionResult, SelectionError> {
    let mut scored = Vec::new();
    let mut healthy_count = 0usize;
    let mut skipped_resources = 0usize;
    let mut skipped_no_telemetry = 0usize;

    for node in input.nodes {
        if !node.healthy {
            continue;
        }
        healthy_count += 1;

        // Need telemetry to score
        let (vcpu_avail, mem_avail) = match (node.available_vcpu, node.available_mem_mib) {
            (Some(v), Some(m)) => (v, m),
            _ => {
                // No telemetry — skip if we have resource requirements,
                // otherwise score as 0 (last resort).
                if input.requirements.is_some() {
                    skipped_no_telemetry += 1;
                    continue;
                }
                // Score 0, but still a candidate
                scored.push(ScoredNode {
                    id: node.id,
                    score: 0.0,
                });
                continue;
            }
        };

        let (pending_vcpu, pending_mem) = input.pending.get(&node.id);

        // Filter by resource requirements
        if let Some(ref reqs) = input.requirements {
            let effective_vcpu = (vcpu_avail as u32).saturating_sub(pending_vcpu);
            let effective_mem = (mem_avail as u64).saturating_sub(pending_mem);

            if effective_vcpu < reqs.vcpu_count || effective_mem < reqs.mem_size_mib as u64 {
                skipped_resources += 1;
                continue;
            }
        }

        let score = compute_score(
            NodeCapacity {
                total_cpu: node.total_cpu as f64,
                total_mem_mib: node.total_mem_mib as f64,
            },
            AvailableResources {
                vcpu: vcpu_avail as f64,
                mem_mib: mem_avail as f64,
            },
            pending_vcpu,
            pending_mem,
            &input.config.weights,
        );

        scored.push(ScoredNode { id: node.id, score });
    }

    if scored.is_empty() {
        if healthy_count == 0 {
            return Err(SelectionError::NoNodes);
        }
        if let Some(ref reqs) = input.requirements {
            return Err(SelectionError::InsufficientResources {
                vcpu_required: reqs.vcpu_count,
                mem_required_mib: reqs.mem_size_mib,
            });
        }
        return Err(SelectionError::NoNodes);
    }

    let ranked = rank_nodes(&mut scored, input.preferred_node_id, input.config);

    Ok(SelectionResult {
        ranked,
        healthy_count,
        skipped_resources,
        skipped_no_telemetry,
    })
}

// ============================================================================
// Internal helpers
// ============================================================================

#[derive(Debug, Clone)]
struct ScoredNode {
    id: Uuid,
    score: f64,
}

/// Rank scored nodes: sticky placement or weighted random, with fallbacks.
fn rank_nodes(
    scored: &mut [ScoredNode],
    preferred_node_id: Option<Uuid>,
    config: &SelectionConfig,
) -> Vec<NodeCandidate> {
    // Sort by score descending
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let max_score = scored.first().map(|n| n.score).unwrap_or(0.0);

    // Sticky placement check
    if let Some(preferred_id) = preferred_node_id {
        if let Some(preferred) = scored.iter().find(|n| n.id == preferred_id) {
            let threshold = max_score * config.sticky_score_threshold;
            if preferred.score >= threshold {
                let mut ranked = Vec::with_capacity(scored.len());
                ranked.push(NodeCandidate {
                    id: preferred_id,
                    score: preferred.score,
                });
                for n in scored.iter() {
                    if n.id != preferred_id {
                        ranked.push(NodeCandidate {
                            id: n.id,
                            score: n.score,
                        });
                    }
                }
                return ranked;
            }
        }
    }

    // No sticky — weighted random for primary, rest in score order as fallbacks
    let primary = weighted_random_pick(scored, max_score, config);

    let mut ranked = Vec::with_capacity(scored.len());
    ranked.push(NodeCandidate {
        id: primary.id,
        score: primary.score,
    });
    for n in scored.iter() {
        if n.id != primary.id {
            ranked.push(NodeCandidate {
                id: n.id,
                score: n.score,
            });
        }
    }
    ranked
}

/// Pick one node via weighted random from the top candidates.
fn weighted_random_pick(
    scored: &[ScoredNode],
    max_score: f64,
    config: &SelectionConfig,
) -> ScoredNode {
    let score_threshold = max_score * config.candidate_score_threshold;
    let mut candidates: Vec<&ScoredNode> = scored
        .iter()
        .filter(|n| n.score >= score_threshold)
        .take(config.max_candidates)
        .collect();

    if candidates.len() < config.min_candidates && scored.len() >= config.min_candidates {
        candidates = scored.iter().take(config.min_candidates).collect();
    } else if candidates.is_empty() && !scored.is_empty() {
        candidates = scored.iter().take(1).collect();
    }

    let total_score: f64 = candidates.iter().map(|n| n.score).sum();

    if total_score <= 0.0 {
        return scored[0].clone();
    }

    let mut rng = rand::rng();
    let random_value: f64 = rng.random::<f64>() * total_score;

    let mut cumulative = 0.0;
    for candidate in &candidates {
        cumulative += candidate.score;
        if random_value <= cumulative {
            return (*candidate).clone();
        }
    }

    scored[0].clone()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SelectionConfig {
        SelectionConfig::default()
    }

    fn empty_pending() -> PendingAllocations {
        PendingAllocations::new()
    }

    fn make_node(
        id: Uuid,
        healthy: bool,
        cpu: i32,
        mem: i64,
        avail_cpu: i32,
        avail_mem: i64,
    ) -> NodeSnapshot {
        NodeSnapshot {
            id,
            healthy,
            total_cpu: cpu,
            total_mem_mib: mem,
            available_vcpu: Some(avail_cpu),
            available_mem_mib: Some(avail_mem),
        }
    }

    fn make_node_no_telemetry(id: Uuid, healthy: bool, cpu: i32, mem: i64) -> NodeSnapshot {
        NodeSnapshot {
            id,
            healthy,
            total_cpu: cpu,
            total_mem_mib: mem,
            available_vcpu: None,
            available_mem_mib: None,
        }
    }

    #[test]
    fn no_nodes_returns_error() {
        let config = default_config();
        let pending = empty_pending();
        let input = SelectionInput {
            nodes: &[],
            preferred_node_id: None,
            requirements: None,
            pending: &pending,
            config: &config,
        };
        assert!(matches!(select_nodes(&input), Err(SelectionError::NoNodes)));
    }

    #[test]
    fn all_unhealthy_returns_no_nodes() {
        let config = default_config();
        let pending = empty_pending();
        let id = Uuid::new_v4();
        let nodes = [make_node(id, false, 16, 32768, 16, 32768)];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: None,
            pending: &pending,
            config: &config,
        };
        assert!(matches!(select_nodes(&input), Err(SelectionError::NoNodes)));
    }

    #[test]
    fn single_healthy_node_is_selected() {
        let config = default_config();
        let pending = empty_pending();
        let id = Uuid::new_v4();
        let nodes = [make_node(id, true, 16, 32768, 8, 16384)];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked.len(), 1);
        assert_eq!(result.ranked[0].id, id);
    }

    #[test]
    fn prefers_less_loaded_node() {
        let config = default_config();
        let pending = empty_pending();
        let busy = Uuid::new_v4();
        let idle = Uuid::new_v4();
        let nodes = [
            make_node(busy, true, 16, 32768, 1, 1024), // ~6% available
            make_node(idle, true, 16, 32768, 14, 30000), // ~90% available
        ];

        // Run many iterations to verify the idle node is picked significantly more
        let mut idle_wins = 0;
        for _ in 0..100 {
            let input = SelectionInput {
                nodes: &nodes,
                preferred_node_id: None,
                requirements: None,
                pending: &pending,
                config: &config,
            };
            let result = select_nodes(&input).unwrap();
            if result.ranked[0].id == idle {
                idle_wins += 1;
            }
        }
        // Idle node should win the vast majority of the time
        assert!(idle_wins > 70, "idle node only won {idle_wins}/100 times");
    }

    #[test]
    fn sticky_placement_prefers_node() {
        let config = default_config();
        let pending = empty_pending();
        let preferred = Uuid::new_v4();
        let other = Uuid::new_v4();
        let nodes = [
            make_node(preferred, true, 16, 32768, 6, 16384), // ~40%
            make_node(other, true, 16, 32768, 8, 20000),     // ~55%
        ];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: Some(preferred),
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        // Preferred is above 20% threshold, should be first
        assert_eq!(result.ranked[0].id, preferred);
    }

    #[test]
    fn sticky_placement_abandoned_when_node_too_loaded() {
        let mut config = default_config();
        config.sticky_score_threshold = 0.8; // Very strict threshold
        let pending = empty_pending();
        let preferred = Uuid::new_v4();
        let other = Uuid::new_v4();
        let nodes = [
            make_node(preferred, true, 16, 32768, 1, 1024), // ~6%
            make_node(other, true, 16, 32768, 14, 30000),   // ~90%
        ];

        // Preferred is way below 80% threshold, so sticky is abandoned.
        // The other node should win the vast majority of weighted random picks.
        let mut other_wins = 0;
        for _ in 0..1000 {
            let input = SelectionInput {
                nodes: &nodes,
                preferred_node_id: Some(preferred),
                requirements: None,
                pending: &pending,
                config: &config,
            };
            let result = select_nodes(&input).unwrap();
            if result.ranked[0].id == other {
                other_wins += 1;
            }
        }
        assert!(
            other_wins > 800,
            "other node only won {other_wins}/1000 times — sticky wasn't properly abandoned"
        );
    }

    #[test]
    fn insufficient_resources_returns_error() {
        let config = default_config();
        let pending = empty_pending();
        let id = Uuid::new_v4();
        let nodes = [make_node(id, true, 16, 32768, 2, 1024)];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(VmRequirements::new(8, 16384)),
            pending: &pending,
            config: &config,
        };
        assert!(matches!(
            select_nodes(&input),
            Err(SelectionError::InsufficientResources { .. })
        ));
    }

    #[test]
    fn resource_requirements_filter_nodes() {
        let config = default_config();
        let pending = empty_pending();
        let small = Uuid::new_v4();
        let big = Uuid::new_v4();
        let nodes = [
            make_node(small, true, 16, 32768, 2, 1024), // not enough
            make_node(big, true, 16, 32768, 12, 24000), // plenty
        ];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(VmRequirements::new(8, 16384)),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked.len(), 1);
        assert_eq!(result.ranked[0].id, big);
        assert_eq!(result.skipped_resources, 1);
    }

    #[test]
    fn pending_allocations_affect_resource_filtering() {
        let config = default_config();
        let mut pending = PendingAllocations::new();
        let id = Uuid::new_v4();
        // Node has 8 vCPUs available, but 6 are pending
        pending.add(id, 6, 0);
        let nodes = [make_node(id, true, 16, 32768, 8, 16384)];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(VmRequirements::new(4, 512)), // Need 4, only 2 effective
            pending: &pending,
            config: &config,
        };
        assert!(matches!(
            select_nodes(&input),
            Err(SelectionError::InsufficientResources { .. })
        ));
    }

    #[test]
    fn no_telemetry_skipped_when_requirements_set() {
        let config = default_config();
        let pending = empty_pending();
        let no_telem = Uuid::new_v4();
        let with_telem = Uuid::new_v4();
        let nodes = [
            make_node_no_telemetry(no_telem, true, 16, 32768),
            make_node(with_telem, true, 16, 32768, 8, 16384),
        ];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(VmRequirements::new(1, 512)),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked.len(), 1);
        assert_eq!(result.ranked[0].id, with_telem);
        assert_eq!(result.skipped_no_telemetry, 1);
    }

    #[test]
    fn no_telemetry_still_candidate_without_requirements() {
        let config = default_config();
        let pending = empty_pending();
        let id = Uuid::new_v4();
        let nodes = [make_node_no_telemetry(id, true, 16, 32768)];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked.len(), 1);
        assert_eq!(result.ranked[0].id, id);
        assert!((result.ranked[0].score - 0.0).abs() < 0.001);
    }

    #[test]
    fn all_candidates_appear_in_result() {
        let config = default_config();
        let pending = empty_pending();
        let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        let nodes: Vec<NodeSnapshot> = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                make_node(
                    id,
                    true,
                    16,
                    32768,
                    (i as i32 + 1) * 2,
                    (i as i64 + 1) * 4096,
                )
            })
            .collect();
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked.len(), 5);
        // All node IDs should appear
        for id in &ids {
            assert!(result.ranked.iter().any(|c| c.id == *id));
        }
    }

    #[test]
    fn sticky_with_unhealthy_preferred_falls_back() {
        let config = default_config();
        let pending = empty_pending();
        let preferred = Uuid::new_v4();
        let other = Uuid::new_v4();
        let nodes = [
            make_node(preferred, false, 16, 32768, 16, 32768), // unhealthy
            make_node(other, true, 16, 32768, 8, 16384),
        ];
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: Some(preferred),
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked[0].id, other);
    }
}
