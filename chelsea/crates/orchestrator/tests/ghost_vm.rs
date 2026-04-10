//! Tests for ghost VM handling in list_all_vms
//!
//! A "ghost VM" is a VM that exists in the orchestrator database but not in chelsea.
//! This can happen due to data inconsistencies. The list_all_vms action should
//! gracefully handle these by soft-deleting the ghost VM record and returning
//! the remaining valid VMs.

use chrono::Utc;
use dto_lib::chelsea_server2::vm::{VmState, VmStatusResponse};
use orch_test::ActionTestEnv;
use orchestrator::{
    db::VMsRepository,
    outbound::node_proto::{HttpError, mock},
};
use uuid::Uuid;

/// Test that ghost VMs (VMs in orch DB but not in chelsea) are:
/// 1. Soft-deleted from the database
/// 2. Not returned in the VM list
/// 3. Don't cause the entire list request to fail
/// 4. Valid VMs are still returned when mixed with ghost VMs
#[test]
fn ghost_vm_handling() {
    ActionTestEnv::with_env(|env| async move {
        // Create VM IDs
        let ghost_vm_id = Uuid::new_v4();
        let valid_vm_id = Uuid::new_v4();

        // Known IDs from seed data (see pg/migrations/20251111063619_seed_db.sql)
        let node_id: Uuid = "4569f1fe-054b-4e8d-855a-f3545167f8a9".parse().unwrap();
        let owner_id: Uuid = "ef90fd52-66b5-47e7-b7dc-e73c4381028f".parse().unwrap();

        // Insert a ghost VM directly into the database
        // This VM won't exist in chelsea, simulating the ghost VM scenario
        env.db()
            .vms()
            .insert(
                ghost_vm_id,
                None, // parent_commit_id
                None, // grandparent_vm_id
                node_id,
                "fd00:fe11:deed:1::99".parse().unwrap(), // ip
                "fake_private_key".to_string(),          // wg_private_key
                "fake_public_key".to_string(),           // wg_public_key
                51899,                                   // wg_port
                owner_id,
                Utc::now(),
                None, // deleted_at (not deleted yet)
                4,
                512,
            )
            .await
            .expect("Failed to insert ghost VM");

        // Insert a valid VM
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

        // Verify both VMs exist and are not deleted
        let ghost_before = env
            .db()
            .vms()
            .get_by_id(ghost_vm_id)
            .await
            .expect("DB error");
        assert!(ghost_before.is_some(), "Ghost VM should exist before test");
        assert!(
            ghost_before.unwrap().deleted_at.is_none(),
            "Ghost VM should not be deleted before test"
        );

        let valid_before = env
            .db()
            .vms()
            .get_by_id(valid_vm_id)
            .await
            .expect("DB error");
        assert!(valid_before.is_some(), "Valid VM should exist before test");

        // Set up mock: ghost VM returns 404, valid VM returns Running
        mock::set_vm_status_mock(move |vm_id| {
            if vm_id == ghost_vm_id {
                // Ghost VM - return 404
                Err(HttpError::NonSuccessStatusCode(
                    404,
                    format!("VM not found: {}", vm_id),
                ))
            } else {
                // Any other VM - return a valid status
                Ok(VmStatusResponse {
                    vm_id: vm_id.to_string(),
                    state: VmState::Running,
                })
            }
        });

        // Call the list VMs endpoint via the test client
        let client =
            orch_test::client::TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        let vms = client
            .vm_list()
            .await
            .expect("List VMs should succeed even with ghost VMs");

        // The ghost VM should NOT be in the returned list
        assert!(
            !vms.iter().any(|vm| vm.vm_id == ghost_vm_id),
            "Ghost VM should not be returned in the list"
        );

        // The valid VM SHOULD be in the returned list
        assert!(
            vms.iter().any(|vm| vm.vm_id == valid_vm_id),
            "Valid VM should be returned in the list"
        );

        // Verify the ghost VM was soft-deleted in the database
        let ghost_after = env
            .db()
            .vms()
            .get_by_id(ghost_vm_id)
            .await
            .expect("DB error");

        // get_by_id filters out deleted VMs, so it should return None
        assert!(
            ghost_after.is_none(),
            "Ghost VM should have been soft-deleted (get_by_id should return None)"
        );

        // Valid VM should still exist
        let valid_after = env
            .db()
            .vms()
            .get_by_id(valid_vm_id)
            .await
            .expect("DB error");
        assert!(valid_after.is_some(), "Valid VM should still exist");

        // Clean up the mock
        mock::clear_vm_status_mock();
    });
}
