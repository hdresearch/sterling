//! Integration tests for per-org resource limits.
//!
//! These tests verify:
//! - `resource_usage()` correctly sums active VM resources for an org
//! - Limits from the `organizations` table are read and enforced
//! - VM creation is rejected when limits would be exceeded
//!
//! Run with: cargo nextest run -p orchestrator --test resource_limits

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use chrono::Utc;
use dto_lib::chelsea_server2::vm::VmCreateVmConfig;
use futures_util::FutureExt;
use orch_test::ActionTestEnv;
use orchestrator::action::{self, ActionError, NewRootVM, NewRootVMError};
use orchestrator::db::{
    ApiKeysRepository, ChelseaNodeRepository, HealthCheckRepository, HealthCheckTelemetry,
    NodeResources, NodeStatus, OrgsRepository, VMsRepository,
};
use tokio::time::timeout;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

macro_rules! resource_limit_test {
    ($name:ident, $timeout_secs:expr, $body:expr) => {
        #[test]
        fn $name() {
            ActionTestEnv::with_env_no_wg(|env| {
                timeout(Duration::from_secs($timeout_secs), async move {
                    #[allow(clippy::redundant_closure_call)]
                    ($body)(env).await;
                })
                .map(|r| r.expect("Test timed out"))
            });
        }
    };
    ($name:ident, $body:expr) => {
        resource_limit_test!($name, 10, $body);
    };
}

/// The seeded org from 20251111063619_seed_db.sql.
fn seed_org_id() -> Uuid {
    "2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d".parse().unwrap()
}

/// The seeded api_key from 20251111063619_seed_db.sql.
fn seed_api_key_id() -> Uuid {
    "ef90fd52-66b5-47e7-b7dc-e73c4381028f".parse().unwrap()
}

/// Insert a node so we can reference it from VMs.
async fn insert_node(env: &ActionTestEnv, node_id: Uuid) {
    let orch_id = *env.orch.id();
    env.db()
        .node()
        .insert(
            node_id,
            &orch_id,
            &NodeResources::new(96, 193024, 1000000, 50),
            "test-privkey",
            "test-pubkey",
            Some(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)),
            Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
        )
        .await
        .expect("insert node");
}

/// Insert a VM owned by the seed api_key with specified resources.
async fn insert_vm_with_resources(
    env: &ActionTestEnv,
    vm_id: Uuid,
    node_id: Uuid,
    vcpu_count: i32,
    mem_size_mib: i32,
) -> Uuid {
    let ip: Ipv6Addr = format!("fd00::1:{}", vm_id.as_fields().0 & 0xFFFF)
        .parse()
        .unwrap_or("fd00::1:1".parse().unwrap());
    env.db()
        .vms()
        .insert(
            vm_id,
            None,
            None,
            node_id,
            ip,
            "pk".into(),
            "pub".into(),
            51820 + (vm_id.as_fields().0 as u16 % 1000),
            seed_api_key_id(),
            Utc::now(),
            None,
            vcpu_count,
            mem_size_mib,
        )
        .await
        .expect("insert vm");
    vm_id
}

/// Insert a VM owned by the seed api_key with default resources (4 vCPUs, 512 MiB).
async fn insert_vm(env: &ActionTestEnv, vm_id: Uuid, node_id: Uuid) -> Uuid {
    insert_vm_with_resources(env, vm_id, node_id, 4, 512).await
}

/// Insert an open usage segment (running VM) with given resources.
async fn insert_usage_segment(env: &ActionTestEnv, vm_id: Uuid, vcpus: u32, ram_mib: u32) {
    let now = Utc::now().timestamp();
    env.db()
        .raw_obj()
        .await
        .execute(
            "INSERT INTO chelsea.vm_usage_segments (vm_id, start_timestamp, start_created_at, vcpu_count, ram_mib)
             VALUES ($1, $2, $3, $4, $5)",
            &[&vm_id, &now, &now, &vcpus, &ram_mib],
        )
        .await
        .expect("insert usage segment");
}

/// Insert a completed (stopped) usage segment.
async fn insert_stopped_segment(env: &ActionTestEnv, vm_id: Uuid, vcpus: u32, ram_mib: u32) {
    let now = Utc::now().timestamp();
    env.db()
        .raw_obj()
        .await
        .execute(
            "INSERT INTO chelsea.vm_usage_segments (vm_id, start_timestamp, start_created_at, vcpu_count, ram_mib, stop_timestamp, stop_created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[&vm_id, &(now - 100), &(now - 100), &vcpus, &ram_mib, &now, &now],
        )
        .await
        .expect("insert stopped usage segment");
}

/// Update the org's user-configurable resource limits.
async fn set_org_limits(env: &ActionTestEnv, org_id: Uuid, max_vcpus: i32, max_memory_mib: i64) {
    env.db()
        .orgs()
        .update_resource_limits(org_id, max_vcpus, max_memory_mib)
        .await
        .expect("set org limits");
}

/// Update the org's admin ceiling (and clamp user limits).
async fn set_admin_limits(env: &ActionTestEnv, org_id: Uuid, max_vcpus: i32, max_memory_mib: i64) {
    env.db()
        .orgs()
        .update_admin_limits(org_id, max_vcpus, max_memory_mib)
        .await
        .expect("set admin limits");
}

// ---------------------------------------------------------------------------
// Tests: resource_usage query
// ---------------------------------------------------------------------------

resource_limit_test!(
    resource_usage_empty_org_returns_zeros,
    |env: &'static ActionTestEnv| async move {
        let usage = env
            .db()
            .orgs()
            .resource_usage(seed_org_id())
            .await
            .expect("resource_usage");

        assert_eq!(usage.vcpus, 0);
        assert_eq!(usage.memory_mib, 0);
    }
);

resource_limit_test!(
    resource_usage_sums_active_vms,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        for _ in 0..3 {
            let vm_id = Uuid::new_v4();
            insert_vm_with_resources(env, vm_id, node_id, 2, 4096).await;
        }

        let usage = env
            .db()
            .orgs()
            .resource_usage(seed_org_id())
            .await
            .expect("resource_usage");

        assert_eq!(usage.vcpus, 6, "3 VMs × 2 vCPUs = 6");
        assert_eq!(usage.memory_mib, 12288, "3 VMs × 4096 MiB = 12288");
    }
);

resource_limit_test!(
    resource_usage_ignores_stopped_vms,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        // resource_usage counts non-deleted VMs with node_id IS NOT NULL.
        // Sleeping VMs (node_id = NULL) are excluded.
        let running = Uuid::new_v4();
        insert_vm_with_resources(env, running, node_id, 4, 8192).await;

        let sleeping = Uuid::new_v4();
        insert_vm_with_resources(env, sleeping, node_id, 8, 16384).await;
        // Put the VM to sleep by clearing its node_id
        env.db()
            .vms()
            .set_node_id(sleeping, None)
            .await
            .expect("set_node_id");

        let usage = env
            .db()
            .orgs()
            .resource_usage(seed_org_id())
            .await
            .expect("resource_usage");

        assert_eq!(usage.vcpus, 4, "only running VM counted");
        assert_eq!(usage.memory_mib, 8192, "only running VM counted");
    }
);

resource_limit_test!(
    resource_usage_ignores_deleted_vms,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        let active = Uuid::new_v4();
        insert_vm_with_resources(env, active, node_id, 2, 1024).await;

        let deleted = Uuid::new_v4();
        insert_vm_with_resources(env, deleted, node_id, 4, 8192).await;
        env.db().vms().mark_deleted(&deleted).await.expect("delete");

        let usage = env
            .db()
            .orgs()
            .resource_usage(seed_org_id())
            .await
            .expect("resource_usage");

        assert_eq!(usage.vcpus, 2, "deleted VM not counted");
        assert_eq!(usage.memory_mib, 1024, "deleted VM not counted");
    }
);

// ---------------------------------------------------------------------------
// Tests: org limits on the entity
// ---------------------------------------------------------------------------

resource_limit_test!(
    org_entity_has_default_limits,
    |env: &'static ActionTestEnv| async move {
        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        assert_eq!(org.admin_max_vcpus(), 8, "default admin_max_vcpus");
        assert_eq!(
            org.admin_max_memory_mib(),
            16384,
            "default admin_max_memory_mib"
        );
        assert_eq!(org.max_vcpus(), 8, "default max_vcpus");
        assert_eq!(org.max_memory_mib(), 16384, "default max_memory_mib");
    }
);

resource_limit_test!(
    user_can_lower_own_limits,
    |env: &'static ActionTestEnv| async move {
        // User lowers their limits below the admin ceiling
        set_org_limits(env, seed_org_id(), 4, 8192).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        assert_eq!(org.max_vcpus(), 4);
        assert_eq!(org.max_memory_mib(), 8192);
        // Admin ceiling unchanged
        assert_eq!(org.admin_max_vcpus(), 8);
        assert_eq!(org.admin_max_memory_mib(), 16384);
    }
);

resource_limit_test!(
    user_cannot_exceed_admin_ceiling,
    |env: &'static ActionTestEnv| async move {
        // Try to set user limits above admin ceiling (8 vCPUs, 16384 MiB)
        let result = env
            .db()
            .orgs()
            .update_resource_limits(seed_org_id(), 100, 409600)
            .await;

        // Should fail due to CHECK constraint
        assert!(
            result.is_err(),
            "should reject user limits above admin ceiling"
        );
    }
);

resource_limit_test!(
    admin_bump_raises_ceiling,
    |env: &'static ActionTestEnv| async move {
        // Admin bumps ceiling (simulating verification)
        set_admin_limits(env, seed_org_id(), 200, 409600).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        assert_eq!(org.admin_max_vcpus(), 200);
        assert_eq!(org.admin_max_memory_mib(), 409600);
        // User limits unchanged (still at old default 8, which is ≤ new ceiling)
        assert_eq!(org.max_vcpus(), 8);
        assert_eq!(org.max_memory_mib(), 16384);

        // Now user can raise their limits up to the new ceiling
        set_org_limits(env, seed_org_id(), 200, 409600).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        assert_eq!(org.max_vcpus(), 200);
        assert_eq!(org.max_memory_mib(), 409600);
    }
);

resource_limit_test!(
    admin_lowering_ceiling_clamps_user_limits,
    |env: &'static ActionTestEnv| async move {
        // First bump admin ceiling high
        set_admin_limits(env, seed_org_id(), 200, 409600).await;
        // User raises their limits
        set_org_limits(env, seed_org_id(), 100, 200000).await;

        // Admin lowers ceiling below user limits
        set_admin_limits(env, seed_org_id(), 50, 100000).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        assert_eq!(org.admin_max_vcpus(), 50);
        assert_eq!(org.admin_max_memory_mib(), 100000);
        // User limits should have been clamped to the new ceiling
        assert_eq!(org.max_vcpus(), 50, "user vcpus should be clamped");
        assert_eq!(
            org.max_memory_mib(),
            100000,
            "user memory should be clamped"
        );
    }
);

// ---------------------------------------------------------------------------
// Tests: check_resource_limits enforcement
// ---------------------------------------------------------------------------

resource_limit_test!(
    check_limits_allows_within_budget,
    |env: &'static ActionTestEnv| async move {
        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        let reqs = orchestrator::action::VmRequirements::new(4, 8192);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_ok(), "should allow within budget");
    }
);

resource_limit_test!(
    check_limits_rejects_vcpu_exceeded,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        // Use 6 of 8 vCPUs
        let vm = Uuid::new_v4();
        insert_vm_with_resources(env, vm, node_id, 6, 1024).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        // Try to add 4 more vCPUs (6 + 4 = 10 > 8 limit)
        let reqs = orchestrator::action::VmRequirements::new(4, 512);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_err(), "should reject — vCPU limit exceeded");

        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("vCPU limit exceeded"), "error message: {msg}");
    }
);

resource_limit_test!(
    check_limits_rejects_memory_exceeded,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        // Use 12288 of 16384 MiB
        let vm = Uuid::new_v4();
        insert_vm_with_resources(env, vm, node_id, 1, 12288).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        // Try to add 8192 MiB (12288 + 8192 = 20480 > 16384 limit)
        let reqs = orchestrator::action::VmRequirements::new(1, 8192);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_err(), "should reject — memory limit exceeded");

        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Memory limit exceeded"),
            "error message: {msg}"
        );
    }
);

resource_limit_test!(
    check_limits_allows_exactly_at_limit,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        // Use 6 of 8 vCPUs, 12288 of 16384 MiB
        let vm = Uuid::new_v4();
        insert_vm_with_resources(env, vm, node_id, 6, 12288).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        // Request exactly what's remaining (2 vCPUs, 4096 MiB)
        let reqs = orchestrator::action::VmRequirements::new(2, 4096);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_ok(), "should allow exactly at limit");
    }
);

resource_limit_test!(
    verification_flow_bumps_limits,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        // Use 6 vCPUs
        let vm = Uuid::new_v4();
        insert_vm_with_resources(env, vm, node_id, 6, 1024).await;

        // With default user limit (8), requesting 4 more should fail
        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        let reqs = orchestrator::action::VmRequirements::new(4, 512);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_err(), "should fail with default limits");

        // Admin bumps ceiling (user verifies)
        set_admin_limits(env, seed_org_id(), 200, 409600).await;
        // User raises their own limits to match
        set_org_limits(env, seed_org_id(), 200, 409600).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_ok(), "should succeed after verification bump");
    }
);

// ---------------------------------------------------------------------------
// Tests: end-to-end via NewRootVM action
// ---------------------------------------------------------------------------

/// Helper to get the seeded API key entity.
async fn get_api_key(env: &ActionTestEnv) -> orchestrator::db::ApiKeyEntity {
    env.db()
        .keys()
        .list_valid()
        .await
        .expect("list keys")
        .into_iter()
        .next()
        .expect("should have at least one key")
}

/// Helper to insert a healthy node so ChooseNode can succeed.
async fn insert_healthy_node(env: &ActionTestEnv, node_id: Uuid) {
    let orch_id = *env.orch.id();
    env.db()
        .node()
        .insert(
            node_id,
            &orch_id,
            &NodeResources::new(96, 193024, 1000000, 50),
            "test-privkey",
            "test-pubkey",
            Some(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)),
            Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
        )
        .await
        .expect("insert node");

    env.db()
        .health()
        .insert(
            node_id,
            NodeStatus::Up,
            Some(HealthCheckTelemetry {
                vcpu_available: Some(80),
                mem_mib_available: Some(160000),
            }),
        )
        .await
        .expect("insert health");
}

resource_limit_test!(
    e2e_new_root_vm_rejected_when_over_limit,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_healthy_node(env, node_id).await;

        let api_key = get_api_key(env).await;

        // Fill up to the limit (8 vCPUs)
        let vm = Uuid::new_v4();
        insert_vm_with_resources(env, vm, node_id, 8, 1024).await;

        // Try to create a VM via the actual action
        let request = VmCreateVmConfig {
            kernel_name: None,
            image_name: None,
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            fs_size_mib: None,
        };

        let result = action::call(NewRootVM::new(request, api_key, false)).await;

        match result {
            Err(ActionError::Error(NewRootVMError::ResourceLimitExceeded(e))) => {
                let msg = e.to_string();
                assert!(msg.contains("vCPU limit exceeded"), "got: {msg}");
            }
            Err(other) => panic!("Expected ResourceLimitExceeded, got: {other:?}"),
            Ok(_) => panic!("Expected rejection but VM creation succeeded"),
        }
    }
);

// ---------------------------------------------------------------------------
// Tests: edge cases
// ---------------------------------------------------------------------------

resource_limit_test!(
    zero_limits_blocks_all_creation,
    |env: &'static ActionTestEnv| async move {
        set_org_limits(env, seed_org_id(), 0, 0).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        // Even a tiny VM should be rejected
        let reqs = orchestrator::action::VmRequirements::new(1, 512);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_err(), "zero limits should block everything");
    }
);

resource_limit_test!(
    deletion_frees_budget,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        // Use all 8 vCPUs across 2 VMs
        let vm1 = Uuid::new_v4();
        insert_vm_with_resources(env, vm1, node_id, 4, 4096).await;

        let vm2 = Uuid::new_v4();
        insert_vm_with_resources(env, vm2, node_id, 4, 4096).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        // Should be at limit — can't add more
        let reqs = orchestrator::action::VmRequirements::new(2, 1024);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_err(), "at limit — should reject");

        // Delete vm2
        env.db().vms().mark_deleted(&vm2).await.expect("delete vm2");

        // Now should have room (4/8 used)
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_ok(), "after deletion — should allow");
    }
);

resource_limit_test!(
    two_vms_individually_fit_but_together_exceed,
    |env: &'static ActionTestEnv| async move {
        let node_id = Uuid::new_v4();
        insert_node(env, node_id).await;

        // Use 2 of 8 vCPUs
        let vm = Uuid::new_v4();
        insert_vm_with_resources(env, vm, node_id, 2, 1024).await;

        let org = env
            .db()
            .orgs()
            .get_by_id(seed_org_id())
            .await
            .expect("get org")
            .expect("org exists");

        // First request for 4 vCPUs: 2 + 4 = 6 ≤ 8 → OK
        let reqs = orchestrator::action::VmRequirements::new(4, 1024);
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_ok(), "first 4 vCPU request should fit");

        // Simulate the first one actually running
        let vm2 = Uuid::new_v4();
        insert_vm_with_resources(env, vm2, node_id, 4, 1024).await;

        // Second request for 4 vCPUs: 2 + 4 + 4 = 10 > 8 → REJECT
        let result: Result<(), orchestrator::action::NewRootVMError> =
            orchestrator::action::vms::check_resource_limits(&env.db(), &org, &reqs).await;
        assert!(result.is_err(), "second 4 vCPU request should exceed limit");
    }
);
