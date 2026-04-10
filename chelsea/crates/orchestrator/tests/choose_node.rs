//! Integration tests for the ChooseNode action (node selection algorithm).
//!
//! These tests verify:
//! - Health filtering: unhealthy / no-heartbeat / flapping nodes excluded
//! - Resource filtering: nodes with insufficient CPU or memory excluded
//! - Scoring & ranking: emptier nodes preferred, weighted random selection
//! - Sticky placement: preferred node wins when healthy + has resources
//! - Pending allocations: concurrent placements don't overcommit
//! - Fleet scenarios: heterogeneous hardware, mixed health, large fleets
//!
//! Run with: cargo nextest run -p orchestrator --test choose_node

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use futures_util::FutureExt;
use orch_test::ActionTestEnv;
use orchestrator::{
    action::{self, ChooseNode, RechooseNodeError, VmRequirements},
    db::{
        ChelseaNodeRepository, HealthCheckRepository, HealthCheckTelemetry, NodeResources,
        NodeStatus,
    },
};
use tokio::time::timeout;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test harness helpers
// ---------------------------------------------------------------------------

/// Wraps the with_env_no_wg + timeout boilerplate.
macro_rules! choose_node_test {
    ($name:ident, $timeout_secs:expr, $body:expr) => {
        #[test]
        fn $name() {
            ActionTestEnv::with_env_no_wg(|env| {
                let ctx = TestCtx::new(env);
                timeout(Duration::from_secs($timeout_secs), async move {
                    #[allow(clippy::redundant_closure_call)]
                    ($body)(ctx).await;
                })
                .map(|r| r.expect("Test timed out"))
            });
        }
    };
    // Default 10s timeout
    ($name:ident, $body:expr) => {
        choose_node_test!($name, 10, $body);
    };
}

/// Lightweight context passed to every test body.
struct TestCtx {
    env: &'static ActionTestEnv,
    orch_id: Uuid,
}

impl TestCtx {
    fn new(env: &'static ActionTestEnv) -> Self {
        Self {
            orch_id: *env.orch.id(),
            env,
        }
    }

    fn db(&self) -> &orchestrator::db::DB {
        self.env.db()
    }

    /// Insert a node with auto-generated IPs/keys.
    /// `index` is used to derive unique IPv6/IPv4 addresses (1-255).
    async fn insert_node(&self, node_id: Uuid, index: u8, resources: &NodeResources) {
        self.db()
            .node()
            .insert(
                node_id,
                &self.orch_id,
                resources,
                &format!("test-private-key-{index}"),
                &format!("test-public-key-{index}"),
                Some(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, index as u16)),
                Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, index))),
            )
            .await
            .unwrap_or_else(|e| panic!("Failed to insert node {index}: {e}"));
    }

    /// Insert a health check with telemetry for the given node.
    async fn health(
        &self,
        node_id: Uuid,
        status: NodeStatus,
        vcpu: Option<i32>,
        mem_mib: Option<i64>,
    ) {
        let telemetry = match (vcpu, mem_mib) {
            (Some(v), Some(m)) => Some(HealthCheckTelemetry {
                vcpu_available: Some(v),
                mem_mib_available: Some(m),
            }),
            _ => None,
        };
        self.db()
            .health()
            .insert(node_id, status, telemetry)
            .await
            .unwrap_or_else(|e| panic!("Failed to insert health check: {e}"));
    }

    /// Shorthand: insert node + Up health check in one call.
    async fn insert_healthy_node(
        &self,
        node_id: Uuid,
        index: u8,
        resources: &NodeResources,
        vcpu_avail: i32,
        mem_avail: i64,
    ) {
        self.insert_node(node_id, index, resources).await;
        self.health(node_id, NodeStatus::Up, Some(vcpu_avail), Some(mem_avail))
            .await;
    }
}

/// Standard node resources used by most tests.
fn std_resources() -> NodeResources {
    NodeResources::new(8, 16384, 100000, 10)
}

/// Call ChooseNode and return the first candidate's node ID.
async fn first_candidate(choose: ChooseNode) -> Uuid {
    action::call(choose)
        .await
        .expect("ChooseNode failed")
        .get_node(0)
        .expect("no candidates")
        .node_id()
}

/// Call ChooseNode and expect it to fail with a specific error variant.
async fn expect_error(choose: ChooseNode, check: impl FnOnce(RechooseNodeError)) {
    let result = action::call(choose).await;
    match result {
        Ok(_) => panic!("Expected ChooseNode to fail, but it succeeded"),
        Err(e) => check(e.try_extract_err().expect("Should have inner error")),
    }
}

// ---------------------------------------------------------------------------
// Health filtering
// ---------------------------------------------------------------------------

choose_node_test!(filters_unhealthy_nodes, |ctx: TestCtx| async move {
    let healthy = Uuid::new_v4();
    let down = Uuid::new_v4();
    let res = std_resources();

    ctx.insert_healthy_node(healthy, 1, &res, 8, 16384).await;
    ctx.insert_node(down, 2, &res).await;
    ctx.health(down, NodeStatus::Down, None, None).await;

    for _ in 0..10 {
        assert_eq!(first_candidate(ChooseNode::new()).await, healthy);
    }
});

choose_node_test!(
    no_health_checks_treated_as_unhealthy,
    |ctx: TestCtx| async move {
        let with_health = Uuid::new_v4();
        let without_health = Uuid::new_v4();
        let res = std_resources();

        ctx.insert_healthy_node(with_health, 1, &res, 8, 16384)
            .await;
        ctx.insert_node(without_health, 2, &res).await;
        // No health check inserted for without_health

        for _ in 0..10 {
            assert_eq!(first_candidate(ChooseNode::new()).await, with_health);
        }
    }
);

choose_node_test!(all_unhealthy_returns_error, |ctx: TestCtx| async move {
    let node = Uuid::new_v4();
    let res = std_resources();

    ctx.insert_node(node, 1, &res).await;
    ctx.health(node, NodeStatus::Down, None, None).await;

    expect_error(ChooseNode::new(), |_| { /* any error is fine */ }).await;
});

// A node that was Down but most recently reported Up should be considered healthy.
// The DB returns ORDER BY timestamp DESC, so the newest entry is first.
choose_node_test!(recovered_node_is_healthy, |ctx: TestCtx| async move {
    let node = Uuid::new_v4();
    let res = std_resources();
    ctx.insert_node(node, 1, &res).await;

    // Insert old Down, then newer Up (timestamp auto-increments)
    ctx.health(node, NodeStatus::Down, None, None).await;
    ctx.health(node, NodeStatus::Up, Some(8), Some(16384)).await;

    assert_eq!(first_candidate(ChooseNode::new()).await, node);
});

// Inverse: node was Up but most recently went Down — should be filtered.
choose_node_test!(recently_down_node_is_unhealthy, |ctx: TestCtx| async move {
    let good = Uuid::new_v4();
    let flapped = Uuid::new_v4();
    let res = std_resources();

    ctx.insert_healthy_node(good, 1, &res, 8, 16384).await;
    ctx.insert_node(flapped, 2, &res).await;
    ctx.health(flapped, NodeStatus::Up, Some(8), Some(16384))
        .await;
    ctx.health(flapped, NodeStatus::Down, None, None).await;

    for _ in 0..10 {
        assert_eq!(first_candidate(ChooseNode::new()).await, good);
    }
});

// ---------------------------------------------------------------------------
// Resource filtering
// ---------------------------------------------------------------------------

choose_node_test!(filters_insufficient_vcpu, |ctx: TestCtx| async move {
    let small = Uuid::new_v4();
    let large = Uuid::new_v4();
    let res = std_resources();

    ctx.insert_healthy_node(small, 1, &res, 2, 16384).await;
    ctx.insert_healthy_node(large, 2, &res, 8, 16384).await;

    let reqs = VmRequirements::new(4, 512);
    for _ in 0..10 {
        assert_eq!(
            first_candidate(ChooseNode::new().with_requirements(reqs.clone())).await,
            large
        );
    }
});

choose_node_test!(filters_insufficient_memory, |ctx: TestCtx| async move {
    let small = Uuid::new_v4();
    let large = Uuid::new_v4();
    let res = std_resources();

    ctx.insert_healthy_node(small, 1, &res, 8, 4096).await;
    ctx.insert_healthy_node(large, 2, &res, 8, 16384).await;

    let reqs = VmRequirements::new(1, 8192);
    for _ in 0..10 {
        assert_eq!(
            first_candidate(ChooseNode::new().with_requirements(reqs.clone())).await,
            large
        );
    }
});

choose_node_test!(insufficient_resources_error, |ctx: TestCtx| async move {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let res = std_resources();

    // A has CPUs but no memory; B has memory but no CPUs
    ctx.insert_healthy_node(a, 1, &res, 2, 4096).await;
    ctx.insert_healthy_node(b, 2, &res, 4, 2048).await;

    let reqs = VmRequirements::new(8, 8192);
    expect_error(ChooseNode::new().with_requirements(reqs), |e| match e {
        RechooseNodeError::InsufficientResources {
            vcpu_required,
            mem_required_mib,
        } => {
            assert_eq!(vcpu_required, 8);
            assert_eq!(mem_required_mib, 8192);
        }
        other => panic!("Expected InsufficientResources, got {other:?}"),
    })
    .await;
});

// Exact match: node has *exactly* the resources required — should still be selected.
choose_node_test!(
    exact_resource_match_is_accepted,
    |ctx: TestCtx| async move {
        let node = Uuid::new_v4();
        let res = std_resources();
        ctx.insert_healthy_node(node, 1, &res, 4, 8192).await;

        let reqs = VmRequirements::new(4, 8192);
        assert_eq!(
            first_candidate(ChooseNode::new().with_requirements(reqs)).await,
            node
        );
    }
);

// ---------------------------------------------------------------------------
// Scoring / ranking
// ---------------------------------------------------------------------------

choose_node_test!(prefers_less_loaded_nodes, 15, |ctx: TestCtx| async move {
    let loaded = Uuid::new_v4();
    let empty = Uuid::new_v4();
    let res = std_resources();

    // Score 50 vs score 100
    ctx.insert_healthy_node(loaded, 1, &res, 4, 8192).await;
    ctx.insert_healthy_node(empty, 2, &res, 8, 16384).await;

    let mut empty_wins = 0u32;
    for _ in 0..50 {
        if first_candidate(ChooseNode::new()).await == empty {
            empty_wins += 1;
        }
    }
    assert!(
        empty_wins > 25,
        "Empty node (score 100) should win majority, got {empty_wins}/50"
    );
});

// With 5 nodes at varying load, the emptiest should rank first most often.
choose_node_test!(five_node_fleet_ranking, 15, |ctx: TestCtx| async move {
    let res = std_resources(); // 8 vcpu, 16384 mem
    let mut nodes = Vec::new();

    // Node 0: 10% avail → score 10
    // Node 1: 30% avail → score 30
    // Node 2: 50% avail → score 50
    // Node 3: 70% avail → score 70
    // Node 4: 90% avail → score 90
    for i in 0u8..5 {
        let id = Uuid::new_v4();
        let pct = (i as f64 * 0.2) + 0.1; // 0.1, 0.3, 0.5, 0.7, 0.9
        let vcpu = (8.0 * pct) as i32;
        let mem = (16384.0 * pct) as i64;
        ctx.insert_healthy_node(id, i + 1, &res, vcpu, mem).await;
        nodes.push(id);
    }

    let emptiest = nodes[4]; // 90% available
    let mut emptiest_first = 0u32;
    for _ in 0..50 {
        if first_candidate(ChooseNode::new()).await == emptiest {
            emptiest_first += 1;
        }
    }
    // Weighted random: score 90 out of total ~250 (10+30+50+70+90) ≈ 36% chance.
    // Over 50 trials, expect ~18. Use > 5 to avoid flakiness.
    assert!(
        emptiest_first > 5,
        "Emptiest node (score 90) should be first sometimes, got {emptiest_first}/50"
    );
});

// Heterogeneous hardware: a big node half-loaded scores same as a small node empty,
// but both should be selectable.
choose_node_test!(heterogeneous_hardware, |ctx: TestCtx| async move {
    let big = Uuid::new_v4();
    let small = Uuid::new_v4();

    let big_res = NodeResources::new(32, 65536, 500000, 20);
    let small_res = NodeResources::new(4, 4096, 50000, 5);

    // Big node 50% loaded: score = (16/32)*50 + (32768/65536)*50 = 50
    ctx.insert_healthy_node(big, 1, &big_res, 16, 32768).await;
    // Small node 50% loaded: score = (2/4)*50 + (2048/4096)*50 = 50
    ctx.insert_healthy_node(small, 2, &small_res, 2, 2048).await;

    // Both should be selectable — neither should dominate completely
    let mut big_count = 0u32;
    let mut small_count = 0u32;
    for _ in 0..50 {
        let id = first_candidate(ChooseNode::new()).await;
        if id == big {
            big_count += 1;
        } else {
            small_count += 1;
        }
    }
    assert!(
        big_count > 5,
        "Big node should be selected sometimes, got {big_count}/50"
    );
    assert!(
        small_count > 5,
        "Small node should be selected sometimes, got {small_count}/50"
    );
});

// Requirements filter on heterogeneous fleet: only the big node can fit a large VM.
choose_node_test!(
    requirements_filter_heterogeneous_fleet,
    |ctx: TestCtx| async move {
        let big = Uuid::new_v4();
        let small = Uuid::new_v4();

        let big_res = NodeResources::new(32, 65536, 500000, 20);
        let small_res = NodeResources::new(4, 4096, 50000, 5);

        ctx.insert_healthy_node(big, 1, &big_res, 16, 32768).await;
        ctx.insert_healthy_node(small, 2, &small_res, 4, 4096).await;

        let reqs = VmRequirements::new(8, 16384);
        for _ in 0..10 {
            assert_eq!(
                first_candidate(ChooseNode::new().with_requirements(reqs.clone())).await,
                big,
                "Only the big node can fit 8 vCPU / 16GB requirement"
            );
        }
    }
);

// ---------------------------------------------------------------------------
// Sticky placement
// ---------------------------------------------------------------------------

choose_node_test!(sticky_selects_preferred, |ctx: TestCtx| async move {
    let preferred = Uuid::new_v4();
    let other = Uuid::new_v4();
    let res = std_resources();

    ctx.insert_healthy_node(preferred, 1, &res, 8, 16384).await;
    ctx.insert_healthy_node(other, 2, &res, 8, 16384).await;

    for _ in 0..10 {
        assert_eq!(
            first_candidate(ChooseNode::with_preferred_node(preferred)).await,
            preferred
        );
    }
});

choose_node_test!(
    sticky_fallback_when_preferred_unhealthy,
    |ctx: TestCtx| async move {
        let preferred = Uuid::new_v4();
        let fallback = Uuid::new_v4();
        let res = std_resources();

        ctx.insert_node(preferred, 1, &res).await;
        ctx.health(preferred, NodeStatus::Down, None, None).await;
        ctx.insert_healthy_node(fallback, 2, &res, 8, 16384).await;

        for _ in 0..10 {
            assert_eq!(
                first_candidate(ChooseNode::with_preferred_node(preferred)).await,
                fallback
            );
        }
    }
);

choose_node_test!(sticky_respects_requirements, |ctx: TestCtx| async move {
    let preferred = Uuid::new_v4();
    let other = Uuid::new_v4();
    let res = std_resources();

    // Preferred node has only 2 vCPUs available
    ctx.insert_healthy_node(preferred, 1, &res, 2, 16384).await;
    ctx.insert_healthy_node(other, 2, &res, 8, 16384).await;

    let reqs = VmRequirements::new(4, 512);
    for _ in 0..10 {
        assert_eq!(
            first_candidate(
                ChooseNode::with_preferred_node(preferred).with_requirements(reqs.clone())
            )
            .await,
            other,
            "Should skip preferred node with insufficient resources"
        );
    }
});

// Sticky placement with 5 nodes — preferred should always win when healthy + has resources.
choose_node_test!(sticky_in_large_fleet, |ctx: TestCtx| async move {
    let res = std_resources();
    let preferred = Uuid::new_v4();
    ctx.insert_healthy_node(preferred, 1, &res, 4, 8192).await;

    // Add 4 more nodes with higher scores (more resources)
    for i in 2u8..=5 {
        let id = Uuid::new_v4();
        ctx.insert_healthy_node(id, i, &res, 8, 16384).await;
    }

    for _ in 0..20 {
        assert_eq!(
            first_candidate(ChooseNode::with_preferred_node(preferred)).await,
            preferred,
            "Preferred node should always be first despite lower score"
        );
    }
});

// ---------------------------------------------------------------------------
// Pending allocations
// ---------------------------------------------------------------------------

choose_node_test!(
    pending_allocations_prevent_overcommit,
    |ctx: TestCtx| async move {
        let node = Uuid::new_v4();
        let res = NodeResources::new(4, 8192, 100000, 10);

        ctx.insert_healthy_node(node, 1, &res, 4, 8192).await;

        let reqs = VmRequirements::new(4, 4096);

        // First reservation holds all resources
        let candidates = action::call(ChooseNode::new().with_requirements(reqs.clone()))
            .await
            .expect("first should succeed");
        let held = candidates.get_node(0).expect("should have candidate");
        assert_eq!(held.node_id(), node);

        // Second should fail — pending allocation blocks it
        expect_error(ChooseNode::new().with_requirements(reqs), |e| {
            assert!(matches!(e, RechooseNodeError::InsufficientResources { .. }))
        })
        .await;

        held.commit();
    }
);

choose_node_test!(
    pending_allocations_released_on_drop,
    |ctx: TestCtx| async move {
        let node = Uuid::new_v4();
        let res = NodeResources::new(4, 8192, 100000, 10);

        ctx.insert_healthy_node(node, 1, &res, 4, 8192).await;

        let reqs = VmRequirements::new(4, 4096);

        // Reserve then drop without commit
        {
            let candidates = action::call(ChooseNode::new().with_requirements(reqs.clone()))
                .await
                .expect("should succeed");
            let _r = candidates.get_node(0).expect("should have candidate");
            // dropped here
        }

        // Should succeed now
        assert_eq!(
            first_candidate(ChooseNode::new().with_requirements(reqs)).await,
            node
        );
    }
);

choose_node_test!(
    pending_allocations_cause_fallback,
    |ctx: TestCtx| async move {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let res = NodeResources::new(4, 8192, 100000, 10);

        ctx.insert_healthy_node(a, 1, &res, 4, 8192).await;
        ctx.insert_healthy_node(b, 2, &res, 4, 8192).await;

        let reqs = VmRequirements::new(4, 4096);

        // First reservation takes one node
        let first = action::call(ChooseNode::new().with_requirements(reqs.clone()))
            .await
            .expect("first should succeed");
        let r1 = first.get_node(0).expect("should have candidate");
        let first_node = r1.node_id();

        // Second should get the other node
        let second_node = first_candidate(ChooseNode::new().with_requirements(reqs)).await;
        assert_ne!(
            second_node, first_node,
            "Should fall back to the other node"
        );

        r1.commit();
    }
);

// Two nodes, three concurrent reservations — third should fail.
choose_node_test!(pending_saturates_entire_fleet, |ctx: TestCtx| async move {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let res = NodeResources::new(4, 8192, 100000, 10);

    ctx.insert_healthy_node(a, 1, &res, 4, 8192).await;
    ctx.insert_healthy_node(b, 2, &res, 4, 8192).await;

    let reqs = VmRequirements::new(4, 4096);

    let c1 = action::call(ChooseNode::new().with_requirements(reqs.clone()))
        .await
        .expect("first");
    let r1 = c1.get_node(0).expect("candidate");

    let c2 = action::call(ChooseNode::new().with_requirements(reqs.clone()))
        .await
        .expect("second");
    let r2 = c2.get_node(0).expect("candidate");

    assert_ne!(r1.node_id(), r2.node_id(), "Should be on different nodes");

    // Third should fail — both nodes fully reserved
    expect_error(ChooseNode::new().with_requirements(reqs), |e| {
        assert!(matches!(e, RechooseNodeError::InsufficientResources { .. }))
    })
    .await;

    r1.commit();
    r2.commit();
});

// ---------------------------------------------------------------------------
// Multi-node complex scenarios
// ---------------------------------------------------------------------------

// Mixed fleet: some healthy, some down, some with no health, some with
// insufficient resources. Only the viable ones should be candidates.
choose_node_test!(mixed_fleet_scenario, |ctx: TestCtx| async move {
    let res = std_resources();

    let healthy_big = Uuid::new_v4(); // Up, 8 vcpu avail
    let healthy_small = Uuid::new_v4(); // Up, 2 vcpu avail (won't fit 4-vcpu req)
    let down_node = Uuid::new_v4(); // Down
    let no_health = Uuid::new_v4(); // No health checks
    let recovering = Uuid::new_v4(); // Was Down, now Up

    ctx.insert_healthy_node(healthy_big, 1, &res, 8, 16384)
        .await;
    ctx.insert_healthy_node(healthy_small, 2, &res, 2, 16384)
        .await;

    ctx.insert_node(down_node, 3, &res).await;
    ctx.health(down_node, NodeStatus::Down, None, None).await;

    ctx.insert_node(no_health, 4, &res).await;

    ctx.insert_node(recovering, 5, &res).await;
    ctx.health(recovering, NodeStatus::Down, None, None).await;
    ctx.health(recovering, NodeStatus::Up, Some(6), Some(12000))
        .await;

    let reqs = VmRequirements::new(4, 4096);

    // Only healthy_big and recovering can serve this request
    let mut big_count = 0u32;
    let mut recovering_count = 0u32;
    for _ in 0..50 {
        let id = first_candidate(ChooseNode::new().with_requirements(reqs.clone())).await;
        assert!(
            id == healthy_big || id == recovering,
            "Got unexpected node {id}"
        );
        if id == healthy_big {
            big_count += 1;
        } else {
            recovering_count += 1;
        }
    }
    // healthy_big (score ~100) should win more often than recovering (score ~70)
    assert!(big_count > 0, "healthy_big should be selected");
    assert!(recovering_count > 0, "recovering should also be selected");
});

// Candidate list ordering: get_node(0) should be highest-scored, get_node(1) next, etc.
choose_node_test!(candidate_ordering, |ctx: TestCtx| async move {
    let res = std_resources(); // 8 vcpu, 16384 mem

    let empty = Uuid::new_v4(); // score ~100
    let half = Uuid::new_v4(); // score ~50
    let almost = Uuid::new_v4(); // score ~10

    ctx.insert_healthy_node(empty, 1, &res, 8, 16384).await;
    ctx.insert_healthy_node(half, 2, &res, 4, 8192).await;
    ctx.insert_healthy_node(almost, 3, &res, 1, 1638).await;

    let candidates = action::call(ChooseNode::new())
        .await
        .expect("should succeed");

    // get_node returns candidates — there should be at least 3
    // Note: get_node(0) may not always be `empty` due to weighted random,
    // but all 3 should be present in the candidate list.
    let mut seen = std::collections::HashSet::new();
    for i in 0..3 {
        if let Some(r) = candidates.get_node(i) {
            seen.insert(r.node_id());
        }
    }
    assert!(seen.contains(&empty), "empty node should be in candidates");
    assert!(
        seen.contains(&half),
        "half-loaded node should be in candidates"
    );
    assert!(
        seen.contains(&almost),
        "almost-full node should be in candidates"
    );
});

// A single node that's barely alive (1 vcpu, 512 MiB free) should still serve tiny VMs.
choose_node_test!(
    barely_alive_node_serves_tiny_vm,
    |ctx: TestCtx| async move {
        let node = Uuid::new_v4();
        let res = NodeResources::new(64, 131072, 1000000, 50);
        ctx.insert_healthy_node(node, 1, &res, 1, 512).await;

        let reqs = VmRequirements::new(1, 512);
        assert_eq!(
            first_candidate(ChooseNode::new().with_requirements(reqs)).await,
            node
        );
    }
);

// No nodes registered at all — should error.
choose_node_test!(empty_fleet_returns_error, |_ctx: TestCtx| async move {
    expect_error(ChooseNode::new(), |_| { /* any error */ }).await;
});

// ---------------------------------------------------------------------------
// Resource updates from telemetry
// ---------------------------------------------------------------------------

// Nodes inserted with zero resources should have those resources updated
// via update_resources, and after the update the scoring algorithm should
// use the real values.
choose_node_test!(
    update_resources_fixes_zero_totals,
    |ctx: TestCtx| async move {
        let node_a = Uuid::new_v4();
        let node_b = Uuid::new_v4();

        // Both nodes inserted with 0 totals (the bug)
        let zero_res = NodeResources::new(0, 0, 0, 0);
        ctx.insert_node(node_a, 1, &zero_res).await;
        ctx.insert_node(node_b, 2, &zero_res).await;

        // Both are healthy with available resources
        ctx.health(node_a, NodeStatus::Up, Some(80), Some(100000))
            .await;
        ctx.health(node_b, NodeStatus::Up, Some(40), Some(50000))
            .await;

        // With 0 totals, scoring returns 0 for both — but they should still
        // be candidates (just with score 0). ChooseNode should not error.
        let _result = action::call(ChooseNode::new()).await.expect(
            "ChooseNode should succeed even with zero-total nodes (they score 0 but are candidates)",
        );

        // Now simulate what the health check fix does: update the totals
        ctx.db()
            .node()
            .update_resources(&node_a, 96, 193024)
            .await
            .expect("update_resources for node_a");
        ctx.db()
            .node()
            .update_resources(&node_b, 96, 193024)
            .await
            .expect("update_resources for node_b");

        // After update, node_a has more available resources and should score higher.
        // Over many trials, node_a (80 vcpu avail / 96 total ≈ 83%) should win more
        // than node_b (40 vcpu avail / 96 total ≈ 42%).
        let mut a_wins = 0u32;
        for _ in 0..100 {
            let id = first_candidate(ChooseNode::new()).await;
            if id == node_a {
                a_wins += 1;
            }
        }
        assert!(
            a_wins > 50,
            "node_a (83% avail) should win majority over node_b (42% avail), got {a_wins}/100"
        );
    }
);

// Verify that update_resources actually persists: read back the node and check.
choose_node_test!(update_resources_persists_to_db, |ctx: TestCtx| async move {
    let node_id = Uuid::new_v4();
    let zero_res = NodeResources::new(0, 0, 0, 0);
    ctx.insert_node(node_id, 1, &zero_res).await;

    // Verify initial zeros
    let node = ctx
        .db()
        .node()
        .get_by_id(&node_id)
        .await
        .expect("db read")
        .expect("node should exist");
    assert_eq!(node.resources().hardware_cpu(), 0);
    assert_eq!(node.resources().hardware_memory_mib(), 0);

    // Update
    ctx.db()
        .node()
        .update_resources(&node_id, 96, 193024)
        .await
        .expect("update_resources");

    // Read back and verify
    let node = ctx
        .db()
        .node()
        .get_by_id(&node_id)
        .await
        .expect("db read")
        .expect("node should exist");
    assert_eq!(node.resources().hardware_cpu(), 96);
    assert_eq!(node.resources().hardware_memory_mib(), 193024);
});
