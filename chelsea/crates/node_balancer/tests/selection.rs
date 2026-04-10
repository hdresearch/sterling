use std::collections::HashMap;

use node_balancer::{
    select_nodes, NodeSnapshot, PendingAllocations, SelectionConfig, SelectionError,
    SelectionInput, VmRequirements,
};
use test_case::test_case;
use uuid::Uuid;

// ============================================================================
// Helpers
// ============================================================================

fn default_config() -> SelectionConfig {
    SelectionConfig::default()
}

fn empty_pending() -> PendingAllocations {
    PendingAllocations::new()
}

fn node(
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

fn node_no_telemetry(id: Uuid, healthy: bool) -> NodeSnapshot {
    NodeSnapshot {
        id,
        healthy,
        total_cpu: 16,
        total_mem_mib: 32768,
        available_vcpu: None,
        available_mem_mib: None,
    }
}

/// Run selection N times and return a histogram of how often each node was ranked first.
fn distribution(
    nodes: &[NodeSnapshot],
    config: &SelectionConfig,
    pending: &PendingAllocations,
    requirements: Option<VmRequirements>,
    preferred: Option<Uuid>,
    iterations: usize,
) -> HashMap<Uuid, usize> {
    let mut counts: HashMap<Uuid, usize> = HashMap::new();
    for _ in 0..iterations {
        let input = SelectionInput {
            nodes,
            preferred_node_id: preferred,
            requirements: requirements.clone(),
            pending,
            config,
        };
        let result = select_nodes(&input).unwrap();
        *counts.entry(result.ranked[0].id).or_default() += 1;
    }
    counts
}

/// Print a histogram to stdout (visible with `cargo test -- --nocapture`).
fn print_histogram(counts: &HashMap<Uuid, usize>, nodes: &[NodeSnapshot], total: usize) {
    let bar_width = 40;
    let max_count = counts.values().copied().max().unwrap_or(1);

    println!("\n  {total} iterations:\n");
    for (i, node) in nodes.iter().enumerate() {
        let count = counts.get(&node.id).copied().unwrap_or(0);
        let pct = (count as f64 / total as f64) * 100.0;
        let bar_len = if max_count > 0 {
            (count as f64 / max_count as f64 * bar_width as f64) as usize
        } else {
            0
        };
        let bar: String = "█".repeat(bar_len);
        let pad: String = " ".repeat(bar_width - bar_len);

        let label = format!(
            "node {i} (cpu:{}/{}, mem:{}/{})",
            node.available_vcpu.unwrap_or(0),
            node.total_cpu,
            node.available_mem_mib.unwrap_or(0),
            node.total_mem_mib,
        );

        println!("  {label:<45} {bar}{pad} {count:>5} ({pct:>5.1}%)");
    }
    println!();
}

/// Assert that every node in `ids` got at least `min_pct`% and at most `max_pct`%
/// of selections out of `total` iterations.
fn assert_distribution_bounds(
    counts: &HashMap<Uuid, usize>,
    ids: &[Uuid],
    nodes: &[NodeSnapshot],
    total: usize,
    min_pct: f64,
    max_pct: f64,
) {
    print_histogram(counts, nodes, total);

    for id in ids {
        let count = counts.get(id).copied().unwrap_or(0);
        let pct = (count as f64 / total as f64) * 100.0;
        assert!(
            pct >= min_pct && pct <= max_pct,
            "node {id}: got {pct:.1}% ({count}/{total}), expected {min_pct}%-{max_pct}%"
        );
    }
}

// ============================================================================
// Error cases
// ============================================================================

#[test]
fn empty_node_list() {
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

#[test_case(1  ; "single unhealthy node")]
#[test_case(5  ; "five unhealthy nodes")]
#[test_case(20 ; "twenty unhealthy nodes")]
fn all_unhealthy(count: usize) {
    let config = default_config();
    let pending = empty_pending();
    let nodes: Vec<_> = (0..count)
        .map(|_| node(Uuid::new_v4(), false, 16, 32768, 16, 32768))
        .collect();
    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: None,
        pending: &pending,
        config: &config,
    };
    assert!(matches!(select_nodes(&input), Err(SelectionError::NoNodes)));
}

#[test_case(8, 16384, 4, 8192  ; "need 4 cpu have 8 but need 8192 mem have 4096")]
#[test_case(4, 512, 2, 16384   ; "need 4 cpu only 2 available")]
#[test_case(1, 32768, 16, 1024 ; "need 32768 mem only 1024 available")]
fn insufficient_resources(req_cpu: u32, req_mem: u32, avail_cpu: i32, avail_mem: i64) {
    let config = default_config();
    let pending = empty_pending();
    let nodes = [node(Uuid::new_v4(), true, 16, 32768, avail_cpu, avail_mem)];
    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: Some(VmRequirements::new(req_cpu, req_mem)),
        pending: &pending,
        config: &config,
    };
    assert!(matches!(
        select_nodes(&input),
        Err(SelectionError::InsufficientResources { .. })
    ));
}

// ============================================================================
// Distribution: equal nodes should get roughly equal traffic
// ============================================================================

#[test_case(2,   100_000, 48.0, 52.0  ; "2 equal nodes")]
#[test_case(3,   100_000, 31.0, 36.0  ; "3 equal nodes")]
#[test_case(5,   100_000, 18.0, 22.0  ; "5 equal nodes")]
#[test_case(10,  100_000,  8.0, 12.0  ; "10 equal nodes")]
fn equal_nodes_even_distribution(node_count: usize, iterations: usize, min_pct: f64, max_pct: f64) {
    let config = default_config();
    let pending = empty_pending();
    let ids: Vec<Uuid> = (0..node_count).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids
        .iter()
        .map(|&id| node(id, true, 16, 32768, 8, 16384))
        .collect();

    let counts = distribution(&nodes, &config, &pending, None, None, iterations);
    assert_distribution_bounds(&counts, &ids, &nodes, iterations, min_pct, max_pct);
}

// ============================================================================
// Distribution: loaded node gets less traffic proportionally
// ============================================================================

#[test]
fn loaded_node_gets_less_traffic() {
    let config = default_config();
    let pending = empty_pending();
    let idle = Uuid::new_v4();
    let busy = Uuid::new_v4();
    let nodes = [
        node(idle, true, 16, 32768, 14, 28672), // ~87% free
        node(busy, true, 16, 32768, 2, 4096),   // ~12% free
    ];

    let n = 100_000;
    let counts = distribution(&nodes, &config, &pending, None, None, n);
    print_histogram(&counts, &nodes, n);

    let idle_pct = (*counts.get(&idle).unwrap_or(&0) as f64 / n as f64) * 100.0;
    let busy_pct = (*counts.get(&busy).unwrap_or(&0) as f64 / n as f64) * 100.0;

    // The idle node should get dramatically more traffic
    assert!(
        idle_pct > busy_pct * 2.0,
        "idle ({idle_pct:.1}%) should get >2x traffic of busy ({busy_pct:.1}%)"
    );
}

#[test]
fn nearly_full_node_almost_never_picked_as_primary() {
    let config = default_config();
    let pending = empty_pending();
    let empty = Uuid::new_v4();
    let nearly_full = Uuid::new_v4();
    let nodes = [
        node(empty, true, 16, 32768, 15, 31744),
        node(nearly_full, true, 16, 32768, 1, 512),
    ];

    let n = 100_000;
    let counts = distribution(&nodes, &config, &pending, None, None, n);
    print_histogram(&counts, &nodes, n);

    let full_count = *counts.get(&nearly_full).unwrap_or(&0);
    let full_pct = (full_count as f64 / n as f64) * 100.0;

    // Nearly-full node is below the 50% candidate threshold so it should
    // only be picked via min_candidates fallback — very rarely
    assert!(
        full_pct < 5.0,
        "nearly full node picked {full_pct:.1}% ({full_count}/{n}), expected <5%"
    );
}

// ============================================================================
// Distribution: heterogeneous node sizes at same utilization %
// ============================================================================

#[test]
fn heterogeneous_sizes_same_utilization_even_distribution() {
    let config = default_config();
    let pending = empty_pending();
    let small = Uuid::new_v4(); // 4 cores
    let medium = Uuid::new_v4(); // 16 cores
    let large = Uuid::new_v4(); // 64 cores

    // All at ~50% utilization — should score equally
    let nodes = [
        node(small, true, 4, 8192, 2, 4096),
        node(medium, true, 16, 32768, 8, 16384),
        node(large, true, 64, 131072, 32, 65536),
    ];

    let ids = [small, medium, large];
    let n = 100_000;
    let counts = distribution(&nodes, &config, &pending, None, None, n);
    assert_distribution_bounds(&counts, &ids, &nodes, n, 31.0, 36.0);
}

// ============================================================================
// Distribution: new_root burst simulation
// ============================================================================

/// Simulate N new_root requests hitting a cluster of equal nodes.
/// After all requests, the distribution should be roughly even.
#[test_case(10, 10000, VmRequirements::new(1, 512)   ; "10 nodes 10000 small vms")]
#[test_case(5,  5000,  VmRequirements::new(2, 1024) ; "5 nodes 5000 medium vms")]
#[test_case(3,  3000,  VmRequirements::new(1, 512)  ; "3 nodes 3000 small vms")]
fn new_root_burst_even_cluster(node_count: usize, vm_count: usize, reqs: VmRequirements) {
    let config = default_config();
    let mut pending = PendingAllocations::new();

    // Each node has 128 vCPUs and 262144 MiB — plenty of headroom for bursts
    let ids: Vec<Uuid> = (0..node_count).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids
        .iter()
        .map(|&id| node(id, true, 128, 262144, 120, 245760))
        .collect();

    let mut placement_counts: HashMap<Uuid, usize> = HashMap::new();

    for i in 0..vm_count {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(reqs.clone()),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        let chosen = result.ranked[0].id;
        *placement_counts.entry(chosen).or_default() += 1;

        // Simulate the reservation: add pending so subsequent requests see it
        pending.add(chosen, reqs.vcpu_count, reqs.mem_size_mib);

        // Simulate periodic health check clearing stale pending allocations
        if i % 50 == 49 {
            for &id in &ids {
                pending.clear_node(&id);
            }
        }
    }

    print_histogram(&placement_counts, &nodes, vm_count);

    // Verify every node got at least 5% of placements (no node was starved)
    for &id in &ids {
        let count = *placement_counts.get(&id).unwrap_or(&0);
        assert!(
            count >= vm_count / 20,
            "node {id}: got {count}/{vm_count} placements — node was starved"
        );
    }
    // Verify no single node hogs more than 60% of placements
    for &id in &ids {
        let count = *placement_counts.get(&id).unwrap_or(&0);
        assert!(
            count <= vm_count * 6 / 10,
            "node {id}: got {count}/{vm_count} placements — too concentrated on one node"
        );
    }
}

/// Simulate a burst where pending tracking prevents thundering herd.
/// Without pending, all 10 requests would pick the same "best" node.
/// With pending, they should spread out.
#[test]
fn pending_prevents_thundering_herd() {
    let config = default_config();
    let mut pending = PendingAllocations::new();

    // 3 identical nodes with plenty of headroom
    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids
        .iter()
        .map(|&id| node(id, true, 128, 262144, 128, 262144))
        .collect();

    let mut placements: HashMap<Uuid, usize> = HashMap::new();
    let reqs = VmRequirements::new(2, 1024);

    // 10000 rapid requests — should spread roughly evenly.
    // We track pending so the algorithm sees load building up, but clear
    // periodically to simulate health check refreshes (avoiding resource exhaustion).
    let n = 10_000usize;
    for i in 0..n {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(reqs.clone()),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        let chosen = result.ranked[0].id;
        *placements.entry(chosen).or_default() += 1;
        pending.add(chosen, reqs.vcpu_count, reqs.mem_size_mib);

        // Simulate periodic health check clearing stale pending allocations
        if i % 50 == 49 {
            for &id in &ids {
                pending.clear_node(&id);
            }
        }
    }

    print_histogram(&placements, &nodes, n);

    // Each node should have gotten at least 10% of placements (no thundering herd)
    // and no single node should have more than 60%
    for &id in &ids {
        let count = *placements.get(&id).unwrap_or(&0);
        assert!(
            count >= n / 10,
            "node {id} got {count}/{n} placements — thundering herd detected"
        );
        assert!(
            count <= n * 6 / 10,
            "node {id} got {count}/{n} placements — too concentrated"
        );
    }
}

// ============================================================================
// Sticky placement
// ============================================================================

#[test]
fn sticky_always_wins_when_above_threshold() {
    let config = default_config();
    let pending = empty_pending();
    let preferred = Uuid::new_v4();
    let other = Uuid::new_v4();

    // Preferred at ~75% free, other at ~87% free — preferred is well above any
    // reasonable threshold (score ~75 vs max ~87, ratio ~0.86)
    let nodes = [
        node(preferred, true, 16, 32768, 12, 24576),
        node(other, true, 16, 32768, 14, 28672),
    ];

    // Sticky is deterministic when above threshold — should always be first
    for _ in 0..100 {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: Some(preferred),
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked[0].id, preferred);
        // Other should be the fallback
        assert_eq!(result.ranked[1].id, other);
    }
}

/// When sticky is accepted, the preferred node is ALWAYS first (deterministic).
/// We test this by running 50 iterations — it must be first every single time.
#[test_case(0.0  ; "0 pct threshold always accepts")]
#[test_case(0.2  ; "20 pct threshold preferred at 25 pct accepted")]
fn sticky_threshold_accepted(threshold: f64) {
    let mut config = default_config();
    config.sticky_score_threshold = threshold;
    let pending = empty_pending();
    let preferred = Uuid::new_v4();
    let best = Uuid::new_v4();

    // Preferred scores ~25, best scores ~100
    let nodes = [
        node(preferred, true, 16, 32768, 4, 8192),
        node(best, true, 16, 32768, 16, 32768),
    ];

    for _ in 0..50 {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: Some(preferred),
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(
            result.ranked[0].id, preferred,
            "preferred should always be first when sticky is accepted"
        );
    }
}

/// When sticky is rejected, the preferred node should NOT dominate.
/// Since it falls to weighted random, the best node should win most of the time.
#[test_case(0.5  ; "50 pct threshold preferred at 25 pct rejected")]
#[test_case(0.8  ; "80 pct threshold preferred at 25 pct rejected")]
#[test_case(1.0  ; "100 pct threshold only accepts best")]
fn sticky_threshold_rejected(threshold: f64) {
    let mut config = default_config();
    config.sticky_score_threshold = threshold;
    let pending = empty_pending();
    let preferred = Uuid::new_v4();
    let best = Uuid::new_v4();

    // Preferred scores ~25, best scores ~100
    let nodes = [
        node(preferred, true, 16, 32768, 4, 8192),
        node(best, true, 16, 32768, 16, 32768),
    ];

    let n = 100_000;
    let counts = distribution(&nodes, &config, &pending, None, Some(preferred), n);
    print_histogram(&counts, &nodes, n);

    let best_pct = (*counts.get(&best).unwrap_or(&0) as f64 / n as f64) * 100.0;
    // Best node (score ~100) should win the vast majority via weighted random
    assert!(
        best_pct > 75.0,
        "best node should win >75% when sticky rejected, got {best_pct:.1}%"
    );
}

#[test]
fn sticky_preferred_unhealthy_falls_back() {
    let config = default_config();
    let pending = empty_pending();
    let preferred = Uuid::new_v4();
    let other = Uuid::new_v4();
    let nodes = [
        node(preferred, false, 16, 32768, 16, 32768),
        node(other, true, 16, 32768, 8, 16384),
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

#[test]
fn sticky_preferred_filtered_by_resources_falls_back() {
    let config = default_config();
    let pending = empty_pending();
    let preferred = Uuid::new_v4();
    let other = Uuid::new_v4();
    let nodes = [
        node(preferred, true, 16, 32768, 1, 512), // not enough
        node(other, true, 16, 32768, 12, 24576),
    ];

    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: Some(preferred),
        requirements: Some(VmRequirements::new(4, 2048)),
        pending: &pending,
        config: &config,
    };
    let result = select_nodes(&input).unwrap();
    assert_eq!(result.ranked[0].id, other);
}

/// Map the exact crossover point where sticky placement loses to load balancing,
/// parameterized by threshold. The best node is always 100% free (score 100).
///
/// We test availability levels at 10% increments plus just-above and just-below
/// the threshold boundary. The boundary is derived from the config: a node with
/// availability% == threshold needs score == threshold * 100, which equals the
/// cutoff (max_score * threshold).
///
/// Sticky wins when: preferred_score >= max_score * threshold
/// Score for a node with X% free (equal CPU and mem): X% * 50 + X% * 50 = X%
/// So sticky wins when: availability% >= threshold
#[test_case(0.2  ; "threshold 20 pct")]
#[test_case(0.33 ; "threshold 33 pct")]
#[test_case(0.4  ; "threshold 40 pct")]
#[test_case(0.5  ; "threshold 50 pct")]
#[test_case(0.7  ; "threshold 70 pct")]
fn sticky_crossover_at_threshold(threshold: f64) {
    let mut config = default_config();
    config.sticky_score_threshold = threshold;
    let pending = empty_pending();

    let total_cpu = 100; // use 100 so availability% maps directly to score
    let total_mem = 100_000i64;

    // Test a range of availability levels
    let test_points: Vec<f64> = {
        let mut pts: Vec<f64> = (0..=10).map(|i| i as f64 * 0.1).collect();
        // Add points just above and below the threshold
        pts.push((threshold + 0.02).min(1.0));
        pts.push((threshold - 0.02).max(0.0));
        pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        pts.dedup();
        pts
    };

    println!("\n  sticky_score_threshold = {threshold}");
    println!(
        "  {:>6}  {:>6}  {:>8}  {}",
        "avail%", "score", "wins/N", "result"
    );
    println!("  {}", "-".repeat(45));

    for &avail_frac in &test_points {
        let avail_cpu = (total_cpu as f64 * avail_frac) as i32;
        let avail_mem = (total_mem as f64 * avail_frac) as i64;

        let preferred = Uuid::new_v4();
        let best = Uuid::new_v4();
        let nodes = [
            node(preferred, true, total_cpu, total_mem, avail_cpu, avail_mem),
            node(best, true, total_cpu, total_mem, total_cpu, total_mem),
        ];

        let n = 1000;
        let mut preferred_first = 0;
        for _ in 0..n {
            let input = SelectionInput {
                nodes: &nodes,
                preferred_node_id: Some(preferred),
                requirements: None,
                pending: &pending,
                config: &config,
            };
            let result = select_nodes(&input).unwrap();
            if result.ranked[0].id == preferred {
                preferred_first += 1;
            }
        }

        let score = avail_frac * 100.0;
        let cutoff = threshold * 100.0;
        let should_win = score >= cutoff;
        let marker = if should_win { "STICKY" } else { "LOAD_BAL" };

        println!(
            "  {:>5.0}%  {:>5.1}  {:>4}/{:<4}  {}",
            avail_frac * 100.0,
            score,
            preferred_first,
            n,
            marker
        );

        if should_win {
            assert_eq!(
                preferred_first,
                n,
                "threshold={threshold}, avail={:.0}%, score={score:.1} >= cutoff={cutoff:.1}: \
                 sticky should win every time, but won {preferred_first}/{n}",
                avail_frac * 100.0
            );
        } else {
            assert!(
                preferred_first < n / 2,
                "threshold={threshold}, avail={:.0}%, score={score:.1} < cutoff={cutoff:.1}: \
                 sticky should lose, but preferred was first {preferred_first}/{n}",
                avail_frac * 100.0
            );
        }
    }
}

#[test]
fn sticky_preferred_not_in_list() {
    let config = default_config();
    let pending = empty_pending();
    let ghost = Uuid::new_v4(); // doesn't exist
    let actual = Uuid::new_v4();
    let nodes = [node(actual, true, 16, 32768, 8, 16384)];

    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: Some(ghost),
        requirements: None,
        pending: &pending,
        config: &config,
    };
    let result = select_nodes(&input).unwrap();
    assert_eq!(result.ranked[0].id, actual);
}

// ============================================================================
// Candidate thresholds and fallbacks
// ============================================================================

#[test]
fn min_candidates_ensures_spread() {
    let mut config = default_config();
    config.candidate_score_threshold = 0.95; // very strict
    config.min_candidates = 2;
    let pending = empty_pending();

    let best = Uuid::new_v4();
    let ok = Uuid::new_v4();
    let bad = Uuid::new_v4();

    // Only `best` passes the 95% threshold, but min_candidates=2 pulls in `ok`
    let nodes = [
        node(best, true, 16, 32768, 16, 32768), // score ~100
        node(ok, true, 16, 32768, 8, 16384),    // score ~50
        node(bad, true, 16, 32768, 1, 1024),    // score ~6
    ];

    let n = 100_000;
    let counts = distribution(&nodes, &config, &pending, None, None, n);
    print_histogram(&counts, &nodes, n);

    // `ok` should get meaningful selections due to min_candidates
    let ok_pct = (*counts.get(&ok).unwrap_or(&0) as f64 / n as f64) * 100.0;
    assert!(
        ok_pct > 20.0,
        "`ok` node should get >20% via min_candidates, got {ok_pct:.1}%"
    );

    // `bad` should never be picked (not in min_candidates)
    let bad_count = *counts.get(&bad).unwrap_or(&0);
    assert!(
        bad_count == 0,
        "`bad` node should not be primary pick, got {bad_count}/{n}"
    );
}

#[test]
fn max_candidates_caps_pool() {
    let mut config = default_config();
    config.max_candidates = 3;
    config.candidate_score_threshold = 0.0; // everyone qualifies
    let pending = empty_pending();

    // 10 identical nodes — only first 3 (by score order, all equal) considered
    let ids: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids
        .iter()
        .map(|&id| node(id, true, 16, 32768, 8, 16384))
        .collect();

    let n = 100_000;
    let counts = distribution(&nodes, &config, &pending, None, None, n);
    print_histogram(&counts, &nodes, n);

    // Since all nodes score equally and they're sorted (unstable), only 3 end
    // up in the candidate pool. The rest should never be primary.
    let selected_nodes: Vec<&Uuid> = counts.keys().collect();
    assert!(
        selected_nodes.len() <= config.max_candidates,
        "expected at most {} unique primary picks, got {}",
        config.max_candidates,
        selected_nodes.len()
    );
}

// ============================================================================
// Telemetry edge cases
// ============================================================================

#[test]
fn mix_of_telemetry_and_no_telemetry_with_requirements() {
    let config = default_config();
    let pending = empty_pending();

    let has_telem = Uuid::new_v4();
    let no_telem = Uuid::new_v4();
    let nodes = [
        node(has_telem, true, 16, 32768, 8, 16384),
        node_no_telemetry(no_telem, true),
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
    assert_eq!(result.ranked[0].id, has_telem);
    assert_eq!(result.skipped_no_telemetry, 1);
}

#[test]
fn no_telemetry_still_candidate_without_requirements() {
    let config = default_config();
    let pending = empty_pending();
    let id = Uuid::new_v4();
    let nodes = [node_no_telemetry(id, true)];

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
    assert!(result.ranked[0].score < 0.001);
}

#[test]
fn all_no_telemetry_with_requirements_is_error() {
    let config = default_config();
    let pending = empty_pending();
    let nodes = [
        node_no_telemetry(Uuid::new_v4(), true),
        node_no_telemetry(Uuid::new_v4(), true),
    ];
    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: Some(VmRequirements::new(1, 512)),
        pending: &pending,
        config: &config,
    };
    // healthy_count=2 but all skipped for no telemetry, with requirements set
    // → should be InsufficientResources (can't verify they have enough)
    assert!(matches!(
        select_nodes(&input),
        Err(SelectionError::InsufficientResources { .. })
    ));
}

// ============================================================================
// Result structure
// ============================================================================

#[test]
fn all_viable_nodes_appear_in_ranked_list() {
    let config = default_config();
    let pending = empty_pending();
    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            node(
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
    for id in &ids {
        assert!(result.ranked.iter().any(|c| c.id == *id));
    }
}

#[test]
fn healthy_count_accurate() {
    let config = default_config();
    let pending = empty_pending();
    let nodes = [
        node(Uuid::new_v4(), true, 16, 32768, 8, 16384),
        node(Uuid::new_v4(), true, 16, 32768, 8, 16384),
        node(Uuid::new_v4(), false, 16, 32768, 8, 16384),
        node(Uuid::new_v4(), false, 16, 32768, 8, 16384),
        node(Uuid::new_v4(), false, 16, 32768, 8, 16384),
    ];
    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: None,
        pending: &pending,
        config: &config,
    };
    let result = select_nodes(&input).unwrap();
    assert_eq!(result.healthy_count, 2);
    assert_eq!(result.ranked.len(), 2);
}

#[test]
fn skipped_resources_count_accurate() {
    let config = default_config();
    let pending = empty_pending();
    let big_enough = Uuid::new_v4();
    let nodes = [
        node(big_enough, true, 16, 32768, 12, 24576),
        node(Uuid::new_v4(), true, 16, 32768, 1, 512),
        node(Uuid::new_v4(), true, 16, 32768, 2, 1024),
    ];
    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: Some(VmRequirements::new(8, 16384)),
        pending: &pending,
        config: &config,
    };
    let result = select_nodes(&input).unwrap();
    assert_eq!(result.skipped_resources, 2);
    assert_eq!(result.ranked.len(), 1);
    assert_eq!(result.ranked[0].id, big_enough);
}

// ============================================================================
// Pending interactions with selection
// ============================================================================

#[test]
fn pending_makes_node_ineligible_for_resource_requirements() {
    let config = default_config();
    let mut pending = PendingAllocations::new();
    let id = Uuid::new_v4();

    // Node has 8 vCPUs free, but 6 are pending → 2 effective
    pending.add(id, 6, 0);
    let nodes = [node(id, true, 16, 32768, 8, 16384)];

    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: Some(VmRequirements::new(4, 512)),
        pending: &pending,
        config: &config,
    };
    assert!(matches!(
        select_nodes(&input),
        Err(SelectionError::InsufficientResources { .. })
    ));
}

#[test]
fn pending_shifts_traffic_to_less_loaded_node() {
    let config = default_config();
    let mut pending = PendingAllocations::new();

    let a = Uuid::new_v4();
    let b = Uuid::new_v4();

    // Both start identical
    let nodes = [
        node(a, true, 16, 32768, 16, 32768),
        node(b, true, 16, 32768, 16, 32768),
    ];

    // Heavy pending on A
    pending.add(a, 12, 24576);

    let n = 100_000;
    let counts = distribution(&nodes, &config, &pending, None, None, n);
    print_histogram(&counts, &nodes, n);

    let b_pct = (*counts.get(&b).unwrap_or(&0) as f64 / n as f64) * 100.0;

    assert!(
        b_pct > 75.0,
        "b should get >75% of traffic when a has heavy pending, got {b_pct:.1}%"
    );
}

// ============================================================================
// Realistic cluster scenarios
// ============================================================================

/// Simulate a production-like scenario: 5-node cluster, mixed sizes,
/// varying load, 50 VM requests with pending tracking.
#[test]
fn realistic_mixed_cluster_50_vms() {
    let config = default_config();
    let mut pending = PendingAllocations::new();

    let nodes_data = [
        // (cpu, mem, avail_cpu, avail_mem)
        (64, 131072, 56, 114688), // big, mostly free
        (64, 131072, 32, 65536),  // big, half loaded
        (32, 65536, 28, 57344),   // medium, mostly free
        (32, 65536, 8, 16384),    // medium, heavily loaded
        (16, 32768, 12, 24576),   // small, mostly free
    ];

    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids
        .iter()
        .zip(nodes_data.iter())
        .map(|(&id, &(cpu, mem, acpu, amem))| node(id, true, cpu, mem, acpu, amem))
        .collect();

    let reqs = VmRequirements::new(1, 512);
    let mut placements: HashMap<Uuid, usize> = HashMap::new();

    for _ in 0..50 {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(reqs.clone()),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        let chosen = result.ranked[0].id;
        *placements.entry(chosen).or_default() += 1;
        pending.add(chosen, reqs.vcpu_count, reqs.mem_size_mib);
    }

    // The heavily loaded node (index 3) should get significantly fewer placements
    let heavy_count = *placements.get(&ids[3]).unwrap_or(&0);
    let free_count = *placements.get(&ids[0]).unwrap_or(&0);
    assert!(
        free_count > heavy_count,
        "free node ({free_count}) should get more than heavy node ({heavy_count})"
    );

    // Mostly-free nodes (indices 0, 2, 4) should each get some placements
    for &i in &[0usize, 2, 4] {
        let count = *placements.get(&ids[i]).unwrap_or(&0);
        assert!(
            count >= 1,
            "mostly-free node {i} got 0 placements — unexpected starvation"
        );
    }

    // Total placements should equal the number of requests
    let total: usize = placements.values().sum();
    assert_eq!(total, 50);
}

/// A cluster that fills up: requests should start failing when resources
/// are exhausted (via pending tracking).
#[test]
fn cluster_fills_up_and_rejects() {
    let config = default_config();
    let mut pending = PendingAllocations::new();

    // 2 nodes with 4 vCPUs each available
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let nodes = [
        node(a, true, 8, 16384, 4, 8192),
        node(b, true, 8, 16384, 4, 8192),
    ];

    let reqs = VmRequirements::new(2, 2048);
    let mut placed = 0;

    for _ in 0..10 {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: Some(reqs.clone()),
            pending: &pending,
            config: &config,
        };
        match select_nodes(&input) {
            Ok(result) => {
                let chosen = result.ranked[0].id;
                pending.add(chosen, reqs.vcpu_count, reqs.mem_size_mib);
                placed += 1;
            }
            Err(SelectionError::InsufficientResources { .. }) => break,
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    // 8 total vCPUs, 2 per VM → 4 VMs max
    assert_eq!(
        placed, 4,
        "should place exactly 4 VMs before exhaustion, placed {placed}"
    );
}

/// After a health check clears pending, capacity is restored.
#[test]
fn health_check_clears_pending_restores_capacity() {
    let config = default_config();
    let mut pending = PendingAllocations::new();

    let id = Uuid::new_v4();
    let nodes = [node(id, true, 8, 16384, 8, 16384)];
    let reqs = VmRequirements::new(2, 2048);

    // Fill up via pending
    for _ in 0..4 {
        pending.add(id, reqs.vcpu_count, reqs.mem_size_mib);
    }

    // Should be full now
    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: Some(reqs.clone()),
        pending: &pending,
        config: &config,
    };
    assert!(select_nodes(&input).is_err());

    // Simulate health check clearing pending
    pending.clear_node(&id);

    // Should succeed again
    let input = SelectionInput {
        nodes: &nodes,
        preferred_node_id: None,
        requirements: Some(reqs.clone()),
        pending: &pending,
        config: &config,
    };
    assert!(select_nodes(&input).is_ok());
}
