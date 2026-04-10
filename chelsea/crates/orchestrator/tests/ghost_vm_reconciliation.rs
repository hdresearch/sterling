//! Tests for the ReconcileGhostVms action - proactive background reconciliation
//!
//! This is in its own test binary because the action system uses a static
//! OnceLock (ACTION_CONTEXT) that can only be initialized once per process.
//! Tests using action::call() must not share a binary with other
//! ActionTestEnv tests.

use chrono::Utc;
use dto_lib::chelsea_server2::vm::{VmState, VmStatusResponse};
use orch_test::ActionTestEnv;
use orchestrator::{
    action::{self, ReconcileGhostVms},
    db::VMsRepository,
    outbound::node_proto::{HttpError, mock},
};
use uuid::Uuid;

/// Test ReconcileGhostVms behavior:
/// - Correctly identifies and soft-deletes ghost VMs (404 from Chelsea)
/// - Leaves valid (Running) VMs untouched
/// - Does NOT delete VMs on transient errors (timeouts, connection refused)
#[test]
fn reconcile_ghost_vms() {
    ActionTestEnv::with_env(|env| async move {
        // --- Scenario 1: ghost VMs are deleted, valid VMs are kept ---

        let ghost_vm_id = Uuid::new_v4();
        let valid_vm_id = Uuid::new_v4();

        // Known IDs from seed data
        let node_id: Uuid = "4569f1fe-054b-4e8d-855a-f3545167f8a9".parse().unwrap();
        let owner_id: Uuid = "ef90fd52-66b5-47e7-b7dc-e73c4381028f".parse().unwrap();

        // Insert a ghost VM (will return 404 from Chelsea)
        env.db()
            .vms()
            .insert(
                ghost_vm_id,
                None,
                None,
                node_id,
                "fd00:fe11:deed:1::99".parse().unwrap(),
                "fake_private_key".to_string(),
                "fake_public_key".to_string(),
                51899,
                owner_id,
                Utc::now(),
                None,
                4,
                512,
            )
            .await
            .expect("Failed to insert ghost VM");

        // Insert a valid VM (will return Running from Chelsea)
        env.db()
            .vms()
            .insert(
                valid_vm_id,
                None,
                None,
                node_id,
                "fd00:fe11:deed:1::98".parse().unwrap(),
                "fake_private_key_2".to_string(),
                "fake_public_key_2".to_string(),
                51898,
                owner_id,
                Utc::now(),
                None,
                4,
                512,
            )
            .await
            .expect("Failed to insert valid VM");

        // Mock: ghost VM returns 404, valid VM returns Running
        mock::set_vm_status_mock(move |vm_id| {
            if vm_id == ghost_vm_id {
                Err(HttpError::NonSuccessStatusCode(
                    404,
                    format!("VM not found: {}", vm_id),
                ))
            } else {
                Ok(VmStatusResponse {
                    vm_id: vm_id.to_string(),
                    state: VmState::Running,
                })
            }
        });

        // Run reconciliation
        let result = action::call(ReconcileGhostVms::new())
            .await
            .expect("ReconcileGhostVms should not return ActionError");

        // Ghost VM should have been deleted
        assert!(
            result.ghost_vms_deleted.contains(&ghost_vm_id),
            "Ghost VM should be in the deleted list"
        );
        assert_eq!(result.ghost_vms_deleted.len(), 1);

        // Verify ghost VM is soft-deleted in DB
        let ghost_after = env
            .db()
            .vms()
            .get_by_id(ghost_vm_id)
            .await
            .expect("DB error");
        assert!(
            ghost_after.is_none(),
            "Ghost VM should have been soft-deleted"
        );

        // Valid VM should still exist
        let valid_after = env
            .db()
            .vms()
            .get_by_id(valid_vm_id)
            .await
            .expect("DB error");
        assert!(valid_after.is_some(), "Valid VM should still exist");

        mock::clear_vm_status_mock();

        // --- Scenario 2: transient errors do NOT cause deletion ---

        let transient_vm_id = Uuid::new_v4();

        env.db()
            .vms()
            .insert(
                transient_vm_id,
                None,
                None,
                node_id,
                "fd00:fe11:deed:1::97".parse().unwrap(),
                "fake_private_key_3".to_string(),
                "fake_public_key_3".to_string(),
                51897,
                owner_id,
                Utc::now(),
                None,
                4,
                512,
            )
            .await
            .expect("Failed to insert VM");

        // Mock: return a timeout error (not a 404)
        mock::set_vm_status_mock(move |_vm_id| Err(HttpError::Timeout));

        let result = action::call(ReconcileGhostVms::new())
            .await
            .expect("ReconcileGhostVms should not return ActionError");

        // VM should NOT have been deleted
        assert!(
            result.ghost_vms_deleted.is_empty(),
            "No VMs should be deleted on transient errors"
        );
        // valid_vm + transient_vm both get timeout errors
        assert!(
            !result.errors.is_empty(),
            "Should record the transient errors"
        );

        // VM should still exist in DB
        let vm_after = env
            .db()
            .vms()
            .get_by_id(transient_vm_id)
            .await
            .expect("DB error");
        assert!(
            vm_after.is_some(),
            "VM should still exist after transient error"
        );

        mock::clear_vm_status_mock();
    });
}
