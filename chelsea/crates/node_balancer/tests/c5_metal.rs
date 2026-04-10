//! Tests modelling the production cluster: homogeneous c5.metal nodes.
//!
//! c5.metal: 96 vCPUs, 192 GiB (196608 MiB) RAM.
//! Default VM: 1 vCPU, 512 MiB.

use std::collections::HashMap;

use node_balancer::{
    select_nodes, NodeSnapshot, PendingAllocations, SelectionConfig, SelectionError,
    SelectionInput, VmRequirements,
};
use test_case::test_case;
use uuid::Uuid;

// ============================================================================
// c5.metal constants
// ============================================================================

const C5_VCPU: i32 = 96;
const C5_MEM_MIB: i64 = 196608; // 192 GiB

// Reserve some capacity for the host OS / hypervisor overhead
const C5_AVAIL_VCPU: i32 = 90;
const C5_AVAIL_MEM_MIB: i64 = 188416; // ~184 GiB

// Default VM size
const DEFAULT_VM_VCPU: u32 = 1;
const DEFAULT_VM_MEM_MIB: u32 = 512;

// ============================================================================
// Helpers
// ============================================================================

fn c5_node(id: Uuid, avail_cpu: i32, avail_mem: i64) -> NodeSnapshot {
    NodeSnapshot {
        id,
        healthy: true,
        total_cpu: C5_VCPU,
        total_mem_mib: C5_MEM_MIB,
        available_vcpu: Some(avail_cpu),
        available_mem_mib: Some(avail_mem),
    }
}

fn c5_node_fresh(id: Uuid) -> NodeSnapshot {
    c5_node(id, C5_AVAIL_VCPU, C5_AVAIL_MEM_MIB)
}

fn c5_node_at_utilization(id: Uuid, pct_used: f64) -> NodeSnapshot {
    let avail_cpu = (C5_AVAIL_VCPU as f64 * (1.0 - pct_used)) as i32;
    let avail_mem = (C5_AVAIL_MEM_MIB as f64 * (1.0 - pct_used)) as i64;
    c5_node(id, avail_cpu, avail_mem)
}

fn default_config() -> SelectionConfig {
    SelectionConfig::default()
}

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

        let avail_cpu = node.available_vcpu.unwrap_or(0);
        let avail_mem_gib = node.available_mem_mib.unwrap_or(0) as f64 / 1024.0;
        let used_pct = 1.0 - (avail_cpu as f64 / C5_AVAIL_VCPU as f64);

        println!(
            "  c5.metal {i} ({avail_cpu:>2}/{C5_VCPU} vCPU, {avail_mem_gib:>5.1}G, {:.0}% used) {bar}{pad} {count:>6} ({pct:>5.1}%)",
            used_pct * 100.0,
        );
    }
    println!();
}

fn run_burst(
    nodes: &[NodeSnapshot],
    config: &SelectionConfig,
    reqs: VmRequirements,
    count: usize,
) -> (HashMap<Uuid, usize>, usize) {
    let mut pending = PendingAllocations::new();
    let mut placements: HashMap<Uuid, usize> = HashMap::new();
    let mut placed = 0;

    for _ in 0..count {
        let input = SelectionInput {
            nodes,
            preferred_node_id: None,
            requirements: Some(reqs.clone()),
            pending: &pending,
            config,
        };
        match select_nodes(&input) {
            Ok(result) => {
                let chosen = result.ranked[0].id;
                *placements.entry(chosen).or_default() += 1;
                pending.add(chosen, reqs.vcpu_count, reqs.mem_size_mib);
                placed += 1;
            }
            Err(_) => break,
        }
    }
    (placements, placed)
}

// ============================================================================
// Equal cluster: N fresh c5.metals, even distribution
// ============================================================================

#[test_case(3,  100_000 ; "3 node cluster")]
#[test_case(5,  100_000 ; "5 node cluster")]
#[test_case(10, 100_000 ; "10 node cluster")]
fn fresh_cluster_even_distribution(node_count: usize, iterations: usize) {
    let config = default_config();
    let pending = PendingAllocations::new();
    let ids: Vec<Uuid> = (0..node_count).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids.iter().map(|&id| c5_node_fresh(id)).collect();

    let mut counts: HashMap<Uuid, usize> = HashMap::new();
    for _ in 0..iterations {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        *counts.entry(result.ranked[0].id).or_default() += 1;
    }

    print_histogram(&counts, &nodes, iterations);

    let expected_pct = 100.0 / node_count as f64;
    for &id in &ids {
        let count = *counts.get(&id).unwrap_or(&0);
        let pct = (count as f64 / iterations as f64) * 100.0;
        assert!(
            (pct - expected_pct).abs() < 2.0,
            "node {id}: {pct:.1}%, expected ~{expected_pct:.1}% (±2%)"
        );
    }
}

// ============================================================================
// Burst: fill a cluster with default VMs via pending tracking
// ============================================================================

/// A 3-node c5.metal cluster should handle a burst of default VMs evenly.
/// 90 avail vCPUs × 3 = 270 vCPUs total. With 1-vCPU VMs and pending tracking,
/// VMs should spread across nodes.
#[test_case(3,  200 ; "3 nodes 200 default vms")]
#[test_case(5,  400 ; "5 nodes 400 default vms")]
#[test_case(10, 500 ; "10 nodes 500 default vms")]
fn default_vm_burst(node_count: usize, vm_count: usize) {
    let config = default_config();
    let ids: Vec<Uuid> = (0..node_count).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids.iter().map(|&id| c5_node_fresh(id)).collect();

    let reqs = VmRequirements::new(DEFAULT_VM_VCPU, DEFAULT_VM_MEM_MIB);
    let (placements, placed) = run_burst(&nodes, &config, reqs, vm_count);

    print_histogram(&placements, &nodes, placed);

    assert_eq!(placed, vm_count, "should place all {vm_count} VMs");

    let expected = vm_count as f64 / node_count as f64;
    for (i, &id) in ids.iter().enumerate() {
        let count = *placements.get(&id).unwrap_or(&0);
        let ratio = count as f64 / expected;
        assert!(
            ratio > 0.5 && ratio < 1.5,
            "node {i}: {count} placements, expected ~{expected:.0} (ratio {ratio:.2})"
        );
    }
}

// ============================================================================
// Capacity limits: how many default VMs fit on a c5.metal cluster?
// ============================================================================

/// The bottleneck for default VMs (1 vCPU, 512 MiB) is vCPUs: 90 per node.
/// Memory: 188416 MiB / 512 MiB = 368 VMs per node (not the bottleneck).
/// So a 3-node cluster should fit ~270 default VMs.
#[test_case(1,  90  ; "1 node max default vms")]
#[test_case(3,  270 ; "3 node max default vms")]
#[test_case(5,  450 ; "5 node max default vms")]
fn cluster_capacity_default_vms(node_count: usize, expected_max: usize) {
    let config = default_config();
    let ids: Vec<Uuid> = (0..node_count).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids.iter().map(|&id| c5_node_fresh(id)).collect();

    let reqs = VmRequirements::new(DEFAULT_VM_VCPU, DEFAULT_VM_MEM_MIB);
    let (_, placed) = run_burst(&nodes, &config, reqs, expected_max + 100);

    assert_eq!(
        placed, expected_max,
        "cluster of {node_count} c5.metals should fit exactly {expected_max} default VMs, placed {placed}"
    );
}

/// Larger VMs (4 vCPU, 8192 MiB) — bottleneck is still vCPUs.
/// 90 / 4 = 22 per node.
#[test_case(1,  22 ; "1 node max 4vcpu vms")]
#[test_case(3,  66 ; "3 node max 4vcpu vms")]
#[test_case(5, 110 ; "5 node max 4vcpu vms")]
fn cluster_capacity_4vcpu_vms(node_count: usize, expected_max: usize) {
    let config = default_config();
    let ids: Vec<Uuid> = (0..node_count).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids.iter().map(|&id| c5_node_fresh(id)).collect();

    let reqs = VmRequirements::new(4, 8192);
    let (_, placed) = run_burst(&nodes, &config, reqs, expected_max + 50);

    assert_eq!(
        placed, expected_max,
        "cluster of {node_count} c5.metals should fit exactly {expected_max} 4-vCPU VMs, placed {placed}"
    );
}

// ============================================================================
// Mixed utilization: nodes at different load levels
// ============================================================================

/// 3-node cluster where one node is heavily loaded. The heavily loaded node
/// should get significantly fewer new placements.
#[test]
fn mixed_load_steers_away_from_busy_node() {
    let config = default_config();
    let pending = PendingAllocations::new();

    let idle = Uuid::new_v4();
    let moderate = Uuid::new_v4();
    let busy = Uuid::new_v4();

    let nodes = [
        c5_node_at_utilization(idle, 0.1),     // 10% used
        c5_node_at_utilization(moderate, 0.5), // 50% used
        c5_node_at_utilization(busy, 0.9),     // 90% used
    ];

    let n = 100_000;
    let mut counts: HashMap<Uuid, usize> = HashMap::new();
    for _ in 0..n {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: None,
            requirements: None,
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        *counts.entry(result.ranked[0].id).or_default() += 1;
    }

    print_histogram(&counts, &nodes, n);

    let idle_pct = (*counts.get(&idle).unwrap_or(&0) as f64 / n as f64) * 100.0;
    let busy_pct = (*counts.get(&busy).unwrap_or(&0) as f64 / n as f64) * 100.0;

    assert!(
        idle_pct > busy_pct * 3.0,
        "idle ({idle_pct:.1}%) should get >3x the traffic of busy ({busy_pct:.1}%)"
    );
}

// ============================================================================
// Branch sticky placement on c5.metals
// ============================================================================

/// When branching, the parent's node should be preferred if it has reasonable
/// capacity, even if another node is slightly less loaded.
#[test]
fn branch_stays_on_parent_node_when_capacity_available() {
    let config = default_config(); // sticky threshold = 0.4
    let pending = PendingAllocations::new();

    let parent_node = Uuid::new_v4();
    let other1 = Uuid::new_v4();
    let other2 = Uuid::new_v4();

    // Parent node at 50% — well above the 40% sticky threshold
    let nodes = [
        c5_node_at_utilization(parent_node, 0.5),
        c5_node_at_utilization(other1, 0.3),
        c5_node_at_utilization(other2, 0.2),
    ];

    // Sticky should be deterministic here
    for _ in 0..100 {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: Some(parent_node),
            requirements: Some(VmRequirements::new(DEFAULT_VM_VCPU, DEFAULT_VM_MEM_MIB)),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        assert_eq!(result.ranked[0].id, parent_node);
    }
}

/// When the parent's node is heavily loaded, the branch should go elsewhere.
#[test]
fn branch_leaves_heavily_loaded_parent_node() {
    let config = default_config();
    let pending = PendingAllocations::new();

    let parent_node = Uuid::new_v4();
    let fresh1 = Uuid::new_v4();
    let fresh2 = Uuid::new_v4();

    let nodes = [
        c5_node_at_utilization(parent_node, 0.95), // 95% used
        c5_node_at_utilization(fresh1, 0.1),
        c5_node_at_utilization(fresh2, 0.1),
    ];

    let n = 1000;
    let mut parent_first = 0;
    for _ in 0..n {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: Some(parent_node),
            requirements: Some(VmRequirements::new(DEFAULT_VM_VCPU, DEFAULT_VM_MEM_MIB)),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        if result.ranked[0].id == parent_node {
            parent_first += 1;
        }
    }

    assert!(
        parent_first == 0,
        "branch should never pick 95%-loaded parent, but did {parent_first}/{n} times"
    );
}

// ============================================================================
// Burst of branches from a single parent VM
// ============================================================================

/// Simulate branching 20 VMs from a single parent. First few should stay
/// on the parent node (sticky), but as it fills up via pending, later
/// branches should spill to other nodes.
#[test]
fn branch_burst_spills_to_other_nodes() {
    let config = default_config();
    let mut pending = PendingAllocations::new();

    let parent_node = Uuid::new_v4();
    let other1 = Uuid::new_v4();
    let other2 = Uuid::new_v4();

    let nodes = [
        c5_node_at_utilization(parent_node, 0.5), // 50% used, 45 vCPUs free
        c5_node_fresh(other1),
        c5_node_fresh(other2),
    ];

    let reqs = VmRequirements::new(2, 4096);
    let mut placements: HashMap<Uuid, usize> = HashMap::new();

    for _ in 0..50 {
        let input = SelectionInput {
            nodes: &nodes,
            preferred_node_id: Some(parent_node),
            requirements: Some(reqs.clone()),
            pending: &pending,
            config: &config,
        };
        let result = select_nodes(&input).unwrap();
        let chosen = result.ranked[0].id;
        *placements.entry(chosen).or_default() += 1;
        pending.add(chosen, reqs.vcpu_count, reqs.mem_size_mib);
    }

    print_histogram(&placements, &nodes, 50);

    let parent_count = *placements.get(&parent_node).unwrap_or(&0);
    let other_count: usize = placements.values().sum::<usize>() - parent_count;

    // Parent should get the first batch (sticky), but not all 50
    assert!(
        parent_count > 0,
        "parent node should get some branches (sticky)"
    );
    assert!(
        other_count > 0,
        "other nodes should get some branches (spill)"
    );
    // With 45 vCPUs free and 2 per VM, parent can fit ~22 before sticky breaks
    assert!(
        parent_count < 40,
        "parent should not get all 50 branches, got {parent_count}"
    );
}

// ============================================================================
// Memory-bottlenecked VMs on c5.metals
// ============================================================================

/// Large-memory VMs where memory is the bottleneck, not CPU.
/// 32 GiB VMs: 184 GiB avail / 32 GiB = 5 per node (not 90 from vCPUs).
#[test_case(1,   5 ; "1 node 32gib vms")]
#[test_case(3,  15 ; "3 nodes 32gib vms")]
#[test_case(5,  25 ; "5 nodes 32gib vms")]
fn cluster_capacity_32gib_vms(node_count: usize, expected_max: usize) {
    let config = default_config();
    let ids: Vec<Uuid> = (0..node_count).map(|_| Uuid::new_v4()).collect();
    let nodes: Vec<_> = ids.iter().map(|&id| c5_node_fresh(id)).collect();

    let reqs = VmRequirements::new(4, 32768); // 4 vCPU, 32 GiB
    let (_, placed) = run_burst(&nodes, &config, reqs, expected_max + 50);

    assert_eq!(
        placed, expected_max,
        "cluster of {node_count} c5.metals should fit {expected_max} 32GiB VMs, placed {placed}"
    );
}
