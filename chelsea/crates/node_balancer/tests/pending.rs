use std::time::Duration;

use node_balancer::PendingAllocations;
use test_case::test_case;
use uuid::Uuid;

// ============================================================================
// Basic operations
// ============================================================================

#[test_case(1, 512   ; "tiny vm")]
#[test_case(4, 2048  ; "medium vm")]
#[test_case(16, 32768 ; "large vm")]
#[test_case(128, 262144 ; "huge vm")]
fn add_and_retrieve(vcpu: u32, mem: u32) {
    let mut pa = PendingAllocations::new();
    let id = Uuid::new_v4();
    pa.add(id, vcpu, mem);
    assert_eq!(pa.get(&id), (vcpu, mem as u64));
}

#[test]
fn accumulates_across_multiple_adds() {
    let mut pa = PendingAllocations::new();
    let id = Uuid::new_v4();
    pa.add(id, 2, 512);
    pa.add(id, 2, 512);
    pa.add(id, 2, 512);
    assert_eq!(pa.get(&id), (6, 1536));
}

#[test]
fn independent_nodes_dont_interfere() {
    let mut pa = PendingAllocations::new();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    pa.add(a, 4, 1024);
    pa.add(b, 8, 2048);
    assert_eq!(pa.get(&a), (4, 1024));
    assert_eq!(pa.get(&b), (8, 2048));
}

// ============================================================================
// Removal
// ============================================================================

#[test_case(4, 1024, 2, 512, 2, 512     ; "partial removal")]
#[test_case(4, 1024, 4, 1024, 0, 0      ; "exact removal")]
#[test_case(4, 1024, 10, 5000, 0, 0     ; "over removal saturates to zero")]
#[test_case(4, 1024, 0, 0, 4, 1024      ; "zero removal is noop")]
fn remove_behavior(
    add_vcpu: u32,
    add_mem: u32,
    rm_vcpu: u32,
    rm_mem: u32,
    exp_vcpu: u32,
    exp_mem: u64,
) {
    let mut pa = PendingAllocations::new();
    let id = Uuid::new_v4();
    pa.add(id, add_vcpu, add_mem);
    pa.remove(id, rm_vcpu, rm_mem);
    assert_eq!(pa.get(&id), (exp_vcpu, exp_mem));
}

#[test]
fn remove_from_unknown_node_is_safe() {
    let mut pa = PendingAllocations::new();
    pa.remove(Uuid::new_v4(), 10, 5000);
    // no panic, no side effects
}

// ============================================================================
// Clear
// ============================================================================

#[test]
fn clear_node_removes_all() {
    let mut pa = PendingAllocations::new();
    let id = Uuid::new_v4();
    pa.add(id, 4, 1024);
    pa.add(id, 4, 1024);
    pa.clear_node(&id);
    assert_eq!(pa.get(&id), (0, 0));
}

#[test]
fn clear_one_node_doesnt_affect_others() {
    let mut pa = PendingAllocations::new();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    pa.add(a, 4, 1024);
    pa.add(b, 8, 2048);
    pa.clear_node(&a);
    assert_eq!(pa.get(&a), (0, 0));
    assert_eq!(pa.get(&b), (8, 2048));
}

#[test]
fn clear_unknown_node_is_safe() {
    let mut pa = PendingAllocations::new();
    pa.clear_node(&Uuid::new_v4());
}

// ============================================================================
// Zero handling
// ============================================================================

#[test]
fn zero_add_is_noop() {
    let mut pa = PendingAllocations::new();
    let id = Uuid::new_v4();
    pa.add(id, 0, 0);
    assert_eq!(pa.get(&id), (0, 0));
}

#[test]
fn unknown_node_returns_zero() {
    let pa = PendingAllocations::new();
    assert_eq!(pa.get(&Uuid::new_v4()), (0, 0));
}

// ============================================================================
// TTL and pruning
// ============================================================================

#[test]
fn prune_removes_stale_entries() {
    let mut pa = PendingAllocations::with_ttl(Duration::from_millis(1));
    let id = Uuid::new_v4();
    pa.add(id, 4, 1024);
    std::thread::sleep(Duration::from_millis(5));
    pa.prune_stale();
    assert_eq!(pa.get(&id), (0, 0));
}

#[test]
fn prune_keeps_fresh_entries() {
    let mut pa = PendingAllocations::with_ttl(Duration::from_secs(60));
    let id = Uuid::new_v4();
    pa.add(id, 4, 1024);
    pa.prune_stale();
    assert_eq!(pa.get(&id), (4, 1024));
}

#[test]
fn prune_mixed_stale_and_fresh() {
    let mut pa = PendingAllocations::with_ttl(Duration::from_millis(10));
    let stale = Uuid::new_v4();
    pa.add(stale, 4, 1024);

    std::thread::sleep(Duration::from_millis(20));

    let fresh = Uuid::new_v4();
    pa.add(fresh, 8, 2048);

    pa.prune_stale();
    assert_eq!(pa.get(&stale), (0, 0));
    assert_eq!(pa.get(&fresh), (8, 2048));
}

// ============================================================================
// Simulated lifecycle
// ============================================================================

/// Simulate the full lifecycle of a VM placement:
/// add pending → success → commit (pending stays until health check clears it)
#[test]
fn lifecycle_successful_placement() {
    let mut pa = PendingAllocations::new();
    let node = Uuid::new_v4();

    // Request comes in, reserve resources
    pa.add(node, 2, 1024);
    assert_eq!(pa.get(&node), (2, 1024));

    // VM placement succeeds — pending intentionally stays
    // (will be cleared by next health check)
    assert_eq!(pa.get(&node), (2, 1024));

    // Health check arrives with fresh telemetry
    pa.clear_node(&node);
    assert_eq!(pa.get(&node), (0, 0));
}

/// Simulate the full lifecycle of a failed VM placement:
/// add pending → failure → remove pending
#[test]
fn lifecycle_failed_placement() {
    let mut pa = PendingAllocations::new();
    let node = Uuid::new_v4();

    pa.add(node, 2, 1024);
    assert_eq!(pa.get(&node), (2, 1024));

    // Placement failed — release the reservation
    pa.remove(node, 2, 1024);
    assert_eq!(pa.get(&node), (0, 0));
}

/// Simulate multiple concurrent requests to the same node.
#[test]
fn lifecycle_concurrent_requests() {
    let mut pa = PendingAllocations::new();
    let node = Uuid::new_v4();

    // 3 concurrent requests reserve resources
    pa.add(node, 2, 1024);
    pa.add(node, 2, 1024);
    pa.add(node, 2, 1024);
    assert_eq!(pa.get(&node), (6, 3072));

    // First succeeds, second fails, third succeeds
    // Second fails → release its resources
    pa.remove(node, 2, 1024);
    assert_eq!(pa.get(&node), (4, 2048));

    // Health check comes in, clears everything
    // (the two successful ones are now reflected in telemetry)
    pa.clear_node(&node);
    assert_eq!(pa.get(&node), (0, 0));
}
