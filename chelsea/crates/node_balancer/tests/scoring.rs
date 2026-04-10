use node_balancer::scoring::{compute_score, compute_score_raw, AvailableResources, NodeCapacity};
use node_balancer::ScoringWeights;
use test_case::test_case;

fn cap(cpu: f64, mem: f64) -> NodeCapacity {
    NodeCapacity {
        total_cpu: cpu,
        total_mem_mib: mem,
    }
}

fn avail(vcpu: f64, mem: f64) -> AvailableResources {
    AvailableResources { vcpu, mem_mib: mem }
}

const W: ScoringWeights = ScoringWeights {
    cpu: 50.0,
    mem: 50.0,
};

// ============================================================================
// Basic score computation (no pending)
// ============================================================================

#[test_case(16.0, 32768.0, 16.0, 32768.0, 100.0 ; "empty node scores 100")]
#[test_case(16.0, 32768.0,  0.0,     0.0,   0.0 ; "full node scores 0")]
#[test_case(16.0, 32768.0,  8.0, 16384.0,  50.0 ; "half utilized scores 50")]
#[test_case(16.0, 32768.0, 12.0, 24576.0,  75.0 ; "25 pct utilized scores 75")]
#[test_case(16.0, 32768.0,  4.0,  8192.0,  25.0 ; "75 pct utilized scores 25")]
fn score_at_utilization(
    total_cpu: f64,
    total_mem: f64,
    avail_cpu: f64,
    avail_mem: f64,
    expected: f64,
) {
    let score = compute_score_raw(cap(total_cpu, total_mem), avail(avail_cpu, avail_mem), &W);
    assert!(
        (score - expected).abs() < 0.01,
        "expected {expected}, got {score}"
    );
}

// ============================================================================
// Heterogeneous nodes at same utilization → same score
// ============================================================================

#[test_case(128.0, 262144.0, 64.0, 131072.0 ; "128 core node at 50 pct")]
#[test_case( 16.0,  32768.0,  8.0,  16384.0 ; "16 core node at 50 pct")]
#[test_case(  4.0,   8192.0,  2.0,   4096.0 ; "4 core node at 50 pct")]
#[test_case( 64.0, 131072.0, 32.0,  65536.0 ; "64 core node at 50 pct")]
fn heterogeneous_same_utilization_same_score(
    total_cpu: f64,
    total_mem: f64,
    avail_cpu: f64,
    avail_mem: f64,
) {
    let score = compute_score_raw(cap(total_cpu, total_mem), avail(avail_cpu, avail_mem), &W);
    assert!((score - 50.0).abs() < 0.01, "expected 50.0, got {score}");
}

// ============================================================================
// Asymmetric weights
// ============================================================================

#[test_case(80.0, 20.0, 16.0, 0.0,  80.0 ; "cpu heavy all cpu free")]
#[test_case(80.0, 20.0,  0.0, 32768.0, 20.0 ; "cpu heavy all mem free")]
#[test_case(20.0, 80.0, 16.0, 0.0,  20.0 ; "mem heavy all cpu free")]
#[test_case(20.0, 80.0,  0.0, 32768.0, 80.0 ; "mem heavy all mem free")]
#[test_case(100.0, 0.0, 8.0, 0.0,  50.0 ; "cpu only half utilized")]
#[test_case(0.0, 100.0, 0.0, 16384.0, 50.0 ; "mem only half utilized")]
fn asymmetric_weights(
    cpu_weight: f64,
    mem_weight: f64,
    avail_cpu: f64,
    avail_mem: f64,
    expected: f64,
) {
    let weights = ScoringWeights {
        cpu: cpu_weight,
        mem: mem_weight,
    };
    let score = compute_score_raw(cap(16.0, 32768.0), avail(avail_cpu, avail_mem), &weights);
    assert!(
        (score - expected).abs() < 0.01,
        "expected {expected}, got {score}"
    );
}

// ============================================================================
// Pending allocation impact
// ============================================================================

#[test_case(0, 0, 100.0 ; "no pending full score")]
#[test_case(4, 8192, 75.0 ; "25 pct pending")]
#[test_case(8, 16384, 50.0 ; "50 pct pending")]
#[test_case(16, 32768, 0.0 ; "100 pct pending zeros out")]
#[test_case(32, 65536, 0.0 ; "over pending clamps to zero")]
fn pending_reduces_score(pending_vcpu: u32, pending_mem: u64, expected: f64) {
    let score = compute_score(
        cap(16.0, 32768.0),
        avail(16.0, 32768.0),
        pending_vcpu,
        pending_mem,
        &W,
    );
    assert!(
        (score - expected).abs() < 0.01,
        "expected {expected}, got {score}"
    );
}

// ============================================================================
// Degenerate inputs
// ============================================================================

#[test_case(0.0, 0.0 ; "zero capacity")]
#[test_case(-1.0, 32768.0 ; "negative cpu")]
#[test_case(16.0, -1.0 ; "negative mem")]
#[test_case(0.0, 32768.0 ; "zero cpu")]
#[test_case(16.0, 0.0 ; "zero mem")]
fn degenerate_capacity_scores_zero(total_cpu: f64, total_mem: f64) {
    let score = compute_score_raw(cap(total_cpu, total_mem), avail(10.0, 10000.0), &W);
    assert!((score - 0.0).abs() < 0.001, "expected 0.0, got {score}");
}

#[test]
fn available_exceeding_total_clamped_to_100() {
    let score = compute_score_raw(cap(16.0, 32768.0), avail(32.0, 65536.0), &W);
    assert!((score - 100.0).abs() < 0.001);
}
