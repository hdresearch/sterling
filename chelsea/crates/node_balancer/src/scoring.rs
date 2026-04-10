//! Node scoring based on resource availability.
//!
//! Scores are percentage-based so heterogeneous nodes compare fairly:
//! a 50%-utilized 128-core node scores the same as a 50%-utilized 16-core node.

/// Weights controlling CPU vs memory contribution to the score.
///
/// Should sum to 100 for the score to remain in [0, 100].
#[derive(Debug, Clone, Copy)]
pub struct ScoringWeights {
    /// Weight given to CPU availability (0-100).
    pub cpu: f64,
    /// Weight given to memory availability (0-100).
    pub mem: f64,
}

/// Default 50/50 weighting.
pub const DEFAULT_WEIGHTS: ScoringWeights = ScoringWeights {
    cpu: 50.0,
    mem: 50.0,
};

/// Resources describing a node's total hardware capacity.
#[derive(Debug, Clone, Copy)]
pub struct NodeCapacity {
    pub total_cpu: f64,
    pub total_mem_mib: f64,
}

/// Resources describing what's currently available on a node.
#[derive(Debug, Clone, Copy)]
pub struct AvailableResources {
    pub vcpu: f64,
    pub mem_mib: f64,
}

/// Compute a node's availability score.
///
/// Higher = more available = better candidate.
///
/// Returns 0.0 if the node has no capacity data or zero hardware resources.
pub fn compute_score(
    capacity: NodeCapacity,
    available: AvailableResources,
    pending_vcpu: u32,
    pending_mem_mib: u64,
    weights: &ScoringWeights,
) -> f64 {
    if capacity.total_cpu <= 0.0 || capacity.total_mem_mib <= 0.0 {
        return 0.0;
    }

    let effective_vcpu = (available.vcpu - pending_vcpu as f64).max(0.0);
    let effective_mem = (available.mem_mib - pending_mem_mib as f64).max(0.0);

    let vcpu_score = (effective_vcpu / capacity.total_cpu) * weights.cpu;
    let mem_score = (effective_mem / capacity.total_mem_mib) * weights.mem;

    (vcpu_score + mem_score).clamp(0.0, 100.0)
}

/// Compute a score without pending adjustments.
pub fn compute_score_raw(
    capacity: NodeCapacity,
    available: AvailableResources,
    weights: &ScoringWeights,
) -> f64 {
    compute_score(capacity, available, 0, 0, weights)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(cpu: f64, mem: f64) -> NodeCapacity {
        NodeCapacity {
            total_cpu: cpu,
            total_mem_mib: mem,
        }
    }

    fn avail(vcpu: f64, mem: f64) -> AvailableResources {
        AvailableResources { vcpu, mem_mib: mem }
    }

    #[test]
    fn empty_node_scores_100() {
        let score = compute_score_raw(cap(16.0, 32768.0), avail(16.0, 32768.0), &DEFAULT_WEIGHTS);
        assert!((score - 100.0).abs() < 0.001);
    }

    #[test]
    fn full_node_scores_0() {
        let score = compute_score_raw(cap(16.0, 32768.0), avail(0.0, 0.0), &DEFAULT_WEIGHTS);
        assert!((score - 0.0).abs() < 0.001);
    }

    #[test]
    fn half_utilized_scores_50() {
        let score = compute_score_raw(cap(16.0, 32768.0), avail(8.0, 16384.0), &DEFAULT_WEIGHTS);
        assert!((score - 50.0).abs() < 0.001);
    }

    #[test]
    fn heterogeneous_nodes_same_utilization_same_score() {
        let big = compute_score_raw(
            cap(128.0, 262144.0),
            avail(64.0, 131072.0),
            &DEFAULT_WEIGHTS,
        );
        let small = compute_score_raw(cap(16.0, 32768.0), avail(8.0, 16384.0), &DEFAULT_WEIGHTS);
        assert!((big - small).abs() < 0.001);
    }

    #[test]
    fn pending_reduces_score() {
        let without = compute_score(
            cap(16.0, 32768.0),
            avail(16.0, 32768.0),
            0,
            0,
            &DEFAULT_WEIGHTS,
        );
        let with = compute_score(
            cap(16.0, 32768.0),
            avail(16.0, 32768.0),
            4,
            8192,
            &DEFAULT_WEIGHTS,
        );
        assert!(with < without);
    }

    #[test]
    fn pending_cant_go_negative() {
        let score = compute_score(
            cap(16.0, 32768.0),
            avail(2.0, 1024.0),
            100,
            999999,
            &DEFAULT_WEIGHTS,
        );
        assert!((score - 0.0).abs() < 0.001);
    }

    #[test]
    fn zero_capacity_scores_0() {
        let score = compute_score_raw(cap(0.0, 0.0), avail(10.0, 10000.0), &DEFAULT_WEIGHTS);
        assert!((score - 0.0).abs() < 0.001);
    }

    #[test]
    fn cpu_heavy_weights() {
        let weights = ScoringWeights {
            cpu: 80.0,
            mem: 20.0,
        };
        // Node with lots of CPU free but little memory
        let score = compute_score_raw(cap(16.0, 32768.0), avail(16.0, 0.0), &weights);
        assert!((score - 80.0).abs() < 0.001);
    }

    #[test]
    fn score_clamped_to_100() {
        // Edge case: available exceeds total (stale telemetry)
        let score = compute_score_raw(cap(16.0, 32768.0), avail(32.0, 65536.0), &DEFAULT_WEIGHTS);
        assert!((score - 100.0).abs() < 0.001);
    }
}
