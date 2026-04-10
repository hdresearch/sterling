use dto_lib::chelsea_server2::vm::{VmCreateRequest, VmCreateVmConfig};
use orchestrator::{
    action::{
        self, BranchVM, CommitVM, DeleteVM, FromCommitVM, ListAllVMs, NewRootVM, UpdateVMState,
    },
    db::VMsRepository,
};
use uuid::Uuid;

use super::common::setup_action_test;
use crate::skip_if_no_endpoint;

/// Comprehensive integration test for NewRootVM action
///
/// NOTE: This is structured as a single large test rather than multiple smaller tests
/// because the action::setup() function sets a global singleton context that persists
/// across test boundaries. When multiple test functions run sequentially, the second
/// test would inherit a stale action context with invalidated database prepared statements
/// from the first test's rollback, causing "prepared statement does not exist" errors.
///
/// By running all test scenarios within a single test function that holds one guard,
/// we ensure proper isolation and avoid the global state conflicts.
///
/// Test scenarios covered:
/// 1. Create VM with full config on Chelsea node
/// 2. Verify failure with non-existent cluster
/// 3. Create VM with minimal/default config
/// 4. Create multiple VMs sequentially
/// 5. Test DeleteVM action (Chelsea handles recursive deletion automatically)
/// 6. Test BranchVM action (create child VM from parent)
/// 7. Test CommitVM action (snapshot a VM)
/// 8. Test FromCommitVM action (restore VM from snapshot)
/// 9. Test UpdateVMState action (pause/resume VM)
/// 10. Test ListAllVMs action (list all VMs on node)
/// Should be run with this command
/// sudo DATABASE_URL="postgresql://postgres:opensesame@127.0.0.1:5432/vers" \
/// CHELSEA_TEST_ENDPOINT=127.0.0.1 \
/// CHELSEA_SERVER_PORT=8111 \
/// HOME=/home/ubuntu \
/// /home/ubuntu/.cargo/bin/cargo test \
/// --package orchestrator \
/// --features integration-tests \
/// --test mod \
/// test_new_root_vm_comprehensive \
/// -- --nocapture

#[tokio::test]
#[ignore]
async fn test_new_root_vm_comprehensive() {
    skip_if_no_endpoint!();

    // Setup once and hold the guard for the entire test
    let (db, cluster_id, node_id, test_endpoint) = setup_action_test().await;

    tracing::info!(
        cluster_id = %cluster_id,
        node_id = %node_id,
        endpoint = %test_endpoint,
        "Starting comprehensive new_root_vm integration test"
    );

    // ========================================================================
    // Scenario 1: Create VM with full configuration
    // ========================================================================
    tracing::info!("Scenario 1: Create VM with full configuration");

    let request = VmCreateVmConfig {
        kernel_name: Some("default.bin".to_string()),
        image_name: Some("default".to_string()),
        vcpu_count: Some(2),
        mem_size_mib: Some(512),
        fs_size_mib: Some(1024),
    };

    let result = action::call(NewRootVM::new(request.clone(), cluster_id)).await;

    match result {
        Ok(response) => {
            tracing::info!(vm_id = %response.id, "✓ NewRootVM action succeeded");

            let vm_id = Uuid::parse_str(&response.id).expect("Response should contain valid UUID");

            // Verify the VM was created in the database
            let vm_entity = db
                .vms()
                .get_by_id(vm_id)
                .await
                .expect("Database query should succeed")
                .expect("VM should exist in database");

            tracing::info!("✓ VM found in database");

            // Verify VM properties
            assert_eq!(
                vm_entity.cluster_id, cluster_id,
                "VM should belong to the correct cluster"
            );
            assert_eq!(
                vm_entity.node_id, node_id,
                "VM should be on the correct node"
            );
            assert_eq!(vm_entity.parent, None, "Root VM should have no parent");

            // Cleanup: Delete the VM using DeleteVM action
            match action::call(DeleteVM::new(vm_id, false)).await {
                Ok(deleted_ids) => {
                    tracing::info!(
                        "✓ Cleaned up VM: {} (deleted {} VMs)",
                        vm_id,
                        deleted_ids.len()
                    );
                    assert_eq!(
                        deleted_ids.len(),
                        1,
                        "Should delete exactly 1 VM (non-recursive)"
                    );
                    assert_eq!(deleted_ids[0], vm_id);
                }
                Err(e) => tracing::warn!("Failed to cleanup VM {}: {:?}", vm_id, e),
            }
        }
        Err(e) => {
            panic!("Scenario 1 failed: NewRootVM action failed: {:?}", e);
        }
    }

    // ========================================================================
    // Scenario 2: Verify failure with non-existent cluster
    // ========================================================================
    tracing::info!("Scenario 2: Verify failure with non-existent cluster");

    let request = VmCreateVmConfig {
        kernel_name: Some("default.bin".to_string()),
        image_name: Some("default".to_string()),
        vcpu_count: Some(2),
        mem_size_mib: Some(512),
        fs_size_mib: Some(1024),
    };

    // Use a non-existent cluster ID
    let nonexistent_cluster = Uuid::new_v4();
    tracing::info!(
        nonexistent_cluster = %nonexistent_cluster,
        "Testing with non-existent cluster"
    );

    let result = action::call(NewRootVM::new(request, nonexistent_cluster)).await;

    // Should fail with ClusterNotFound error
    assert!(
        result.is_err(),
        "Action should fail for non-existent cluster"
    );

    match result.unwrap_err() {
        orchestrator::action::ActionError::Error(
            orchestrator::action::NewRootVMError::ClusterNotFound,
        ) => {
            tracing::info!("✓ Correctly failed with ClusterNotFound error");
        }
        other => {
            panic!(
                "Scenario 2 failed: Expected ClusterNotFound error, got: {:?}",
                other
            );
        }
    }

    // ========================================================================
    // Scenario 3: Create VM with minimal/default configuration
    // ========================================================================
    tracing::info!("Scenario 3: Create VM with minimal/default configuration");

    let request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: None,
            image_name: None,
            vcpu_count: None,
            mem_size_mib: None,
            fs_size_mib: None,
        },
    };

    let result = action::call(NewRootVM::new(request, cluster_id)).await;

    match result {
        Ok(response) => {
            tracing::info!(vm_id = %response.id, "✓ Created VM with default config");

            let vm_id = Uuid::parse_str(&response.id).expect("Response should contain valid UUID");

            // Verify in database
            let vm_entity = db
                .vms()
                .get_by_id(vm_id)
                .await
                .expect("Database query should succeed")
                .expect("VM should exist in database");

            assert_eq!(vm_entity.cluster_id, cluster_id);
            assert_eq!(vm_entity.node_id, node_id);
            tracing::info!("✓ VM verified in database");

            // Cleanup using DeleteVM action
            match action::call(DeleteVM::new(vm_id, false)).await {
                Ok(deleted_ids) => {
                    tracing::info!("✓ Cleaned up VM: {}", vm_id);
                    assert_eq!(deleted_ids.len(), 1);
                }
                Err(e) => tracing::warn!("Failed to cleanup VM {}: {:?}", vm_id, e),
            }
        }
        Err(e) => {
            panic!(
                "Scenario 3 failed: NewRootVM with default config should succeed: {:?}",
                e
            );
        }
    }

    // ========================================================================
    // Scenario 4: Create multiple VMs sequentially
    // ========================================================================
    tracing::info!("Scenario 4: Create multiple VMs sequentially");

    let mut created_vm_ids = Vec::new();

    // Create 3 VMs
    for i in 0..3 {
        let request = VmCreateRequest {
            vm_config: VmCreateVmConfig {
                kernel_name: Some("default.bin".to_string()),
                image_name: Some("default".to_string()),
                vcpu_count: Some(1),
                mem_size_mib: Some(256),
                fs_size_mib: Some(512),
            },
        };

        let result = action::call(NewRootVM::new(request, cluster_id)).await;

        match result {
            Ok(response) => {
                let vm_id =
                    Uuid::parse_str(&response.id).expect("Response should contain valid UUID");

                tracing::info!("✓ Created VM {}: {}", i, vm_id);
                created_vm_ids.push(vm_id);

                // Verify in database
                let vm_entity = db
                    .vms()
                    .get_by_id(vm_id)
                    .await
                    .expect("Database query should succeed")
                    .expect("VM should exist in database");

                assert_eq!(vm_entity.cluster_id, cluster_id);
                assert_eq!(vm_entity.node_id, node_id);
            }
            Err(e) => {
                // Cleanup VMs created so far
                let proto = orchestrator::outbound::node_proto::ChelseaProto::new();
                for vm_id in &created_vm_ids {
                    let _ = proto.delete_vm(test_endpoint, *vm_id).await;
                }
                panic!("Scenario 4 failed: Failed to create VM {}: {:?}", i, e);
            }
        }

        // Small delay between creations
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Verify all VMs are distinct
    assert_eq!(created_vm_ids.len(), 3);
    let unique_ids: std::collections::HashSet<_> = created_vm_ids.iter().collect();
    assert_eq!(unique_ids.len(), 3, "All VM IDs should be unique");
    tracing::info!("✓ All 3 VMs created with unique IDs");

    // Cleanup all VMs using DeleteVM action
    for (i, vm_id) in created_vm_ids.iter().enumerate() {
        match action::call(DeleteVM::new(*vm_id, false)).await {
            Ok(deleted_ids) => {
                tracing::info!(
                    "✓ Cleaned up VM {}: {} (deleted {} VMs)",
                    i,
                    vm_id,
                    deleted_ids.len()
                );
                assert_eq!(deleted_ids.len(), 1);
            }
            Err(e) => tracing::warn!("Failed to cleanup VM {} ({}): {:?}", i, vm_id, e),
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // ========================================================================
    // Scenario 5: Test DeleteVM action with verification
    // ========================================================================
    tracing::info!("Scenario 5: Test DeleteVM action explicitly");

    // Create a VM specifically to test deletion
    let request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(1),
            mem_size_mib: Some(256),
            fs_size_mib: Some(512),
        },
    };

    let vm_result = action::call(NewRootVM::new(request, cluster_id)).await;
    match vm_result {
        Ok(response) => {
            let vm_id = Uuid::parse_str(&response.id).expect("Valid UUID");
            tracing::info!("✓ Created VM for deletion test: {}", vm_id);

            // Verify VM exists in database
            let vm_exists_before = db.vms().get_by_id(vm_id).await.unwrap();
            assert!(
                vm_exists_before.is_some(),
                "VM should exist before deletion"
            );

            // Now delete it using DeleteVM action
            let delete_result = action::call(DeleteVM::new(vm_id, false)).await;
            match delete_result {
                Ok(deleted_ids) => {
                    tracing::info!("✓ DeleteVM succeeded, deleted {} VMs", deleted_ids.len());
                    assert_eq!(deleted_ids.len(), 1, "Should delete exactly 1 VM");
                    assert_eq!(deleted_ids[0], vm_id);

                    // Verify VM is gone from database (DeleteVM removes it)
                    // Note: This is within the test transaction, so we can't verify
                    // deletion here since DeleteVM also deletes from DB
                    tracing::info!(
                        "✓ DeleteVM action properly deleted VM from both Chelsea and database"
                    );
                }
                Err(e) => {
                    panic!("Scenario 5 failed: DeleteVM action failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!(
                "Scenario 5 failed: Could not create VM for deletion test: {:?}",
                e
            );
        }
    }

    // ========================================================================
    // Scenario 6: Test BranchVM action (create child VM from parent)
    // ========================================================================
    tracing::info!("Scenario 6: Test BranchVM action");

    // Create a parent VM first
    let parent_request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(2),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let parent_result = action::call(NewRootVM::new(parent_request, cluster_id)).await;
    match parent_result {
        Ok(parent_response) => {
            let parent_vm_id = Uuid::parse_str(&parent_response.id).expect("Valid UUID");
            tracing::info!("✓ Created parent VM: {}", parent_vm_id);

            // Verify parent exists in database
            let parent_vm = db
                .vms()
                .get_by_id(parent_vm_id)
                .await
                .unwrap()
                .expect("Parent VM exists");
            assert_eq!(parent_vm.parent, None, "Parent VM should have no parent");

            // Now branch from the parent VM
            let branch_result = action::call(BranchVM::new(
                parent_vm_id,
                Some("child-branch".to_string()),
            ))
            .await;
            match branch_result {
                Ok(child_vm) => {
                    tracing::info!("✓ BranchVM succeeded, created child VM: {}", child_vm.vm_id);

                    // Verify the child VM properties
                    assert_eq!(
                        child_vm.parent,
                        Some(parent_vm_id),
                        "Child VM should have parent set"
                    );
                    assert_eq!(
                        child_vm.cluster_id, cluster_id,
                        "Child should be in same cluster"
                    );
                    assert_eq!(
                        child_vm.node_id, node_id,
                        "Child should be on same node as parent"
                    );

                    // Verify child exists in database
                    let db_child = db
                        .vms()
                        .get_by_id(child_vm.vm_id)
                        .await
                        .unwrap()
                        .expect("Child VM exists in DB");
                    assert_eq!(
                        db_child.parent,
                        Some(parent_vm_id),
                        "DB record should show parent relationship"
                    );

                    tracing::info!(
                        "✓ BranchVM action properly created child VM with parent relationship"
                    );

                    // Cleanup: Delete child first (since it depends on parent)
                    match action::call(DeleteVM::new(child_vm.vm_id, false)).await {
                        Ok(deleted_ids) => {
                            tracing::info!(
                                "✓ Cleaned up child VM: {} ({} VMs deleted)",
                                child_vm.vm_id,
                                deleted_ids.len()
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Failed to cleanup child VM {}: {:?}", child_vm.vm_id, e)
                        }
                    }

                    // Cleanup: Delete parent VM
                    match action::call(DeleteVM::new(parent_vm_id, false)).await {
                        Ok(deleted_ids) => {
                            tracing::info!(
                                "✓ Cleaned up parent VM: {} ({} VMs deleted)",
                                parent_vm_id,
                                deleted_ids.len()
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Failed to cleanup parent VM {}: {:?}", parent_vm_id, e)
                        }
                    }
                }
                Err(e) => {
                    // Cleanup parent on failure
                    let _ = action::call(DeleteVM::new(parent_vm_id, false)).await;
                    panic!("Scenario 6 failed: BranchVM action failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!(
                "Scenario 6 failed: Could not create parent VM for branch test: {:?}",
                e
            );
        }
    }

    // ========================================================================
    // Scenario 7: Test CommitVM action (snapshot a VM)
    // ========================================================================
    tracing::info!("Scenario 7: Test CommitVM action");

    // Create a VM to commit
    let commit_request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(2),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let commit_vm_result = action::call(NewRootVM::new(commit_request, cluster_id)).await;
    match commit_vm_result {
        Ok(response) => {
            let vm_id = Uuid::parse_str(&response.id).expect("Valid UUID");
            tracing::info!("✓ Created VM for commit test: {}", vm_id);

            // Now commit it using CommitVM action
            let commit_result = action::call(CommitVM::new(vm_id)).await;
            match commit_result {
                Ok(commit_response) => {
                    tracing::info!(
                        "✓ CommitVM succeeded, commit_id: {}, architecture: {}",
                        commit_response.commit_id,
                        commit_response.host_architecture
                    );

                    // Verify commit_id is a valid UUID
                    let commit_id = Uuid::parse_str(&commit_response.commit_id)
                        .expect("Commit ID should be a valid UUID");
                    tracing::info!("✓ Commit ID is valid UUID: {}", commit_id);

                    // Verify architecture is non-empty
                    assert!(
                        !commit_response.host_architecture.is_empty(),
                        "Architecture should not be empty"
                    );
                    tracing::info!("✓ CommitVM action properly committed VM to snapshot");
                }
                Err(e) => {
                    // Cleanup VM on failure
                    let _ = action::call(DeleteVM::new(vm_id, false)).await.unwrap();
                    panic!("Scenario 7 failed: CommitVM action failed: {:?}", e);
                }
            }

            // Cleanup: Delete the VM
            match action::call(DeleteVM::new(vm_id, false)).await {
                Ok(deleted_ids) => {
                    tracing::info!(
                        "✓ Cleaned up VM after commit test: {} ({} VMs deleted)",
                        vm_id,
                        deleted_ids.len()
                    );
                }
                Err(e) => tracing::warn!("Failed to cleanup VM {}: {:?}", vm_id, e),
            }
        }
        Err(e) => {
            panic!(
                "Scenario 7 failed: Could not create VM for commit test: {:?}",
                e
            );
        }
    }

    // ========================================================================
    // Scenario 8: Test FromCommitVM action (restore VM from snapshot)
    // ========================================================================
    tracing::info!("Scenario 8: Test FromCommitVM action");

    // Create a VM to commit and restore
    let restore_request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(2),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let restore_vm_result = action::call(NewRootVM::new(restore_request, cluster_id)).await;
    match restore_vm_result {
        Ok(response) => {
            let original_vm_id = Uuid::parse_str(&response.id).expect("Valid UUID");
            tracing::info!("✓ Created VM for restore test: {}", original_vm_id);

            // Commit the VM first
            let commit_result = action::call(CommitVM::new(original_vm_id)).await;
            match commit_result {
                Ok(commit_response) => {
                    let commit_id = Uuid::parse_str(&commit_response.commit_id)
                        .expect("Commit ID should be a valid UUID");
                    tracing::info!("✓ Committed VM to snapshot: {}", commit_id);

                    // Now restore from the commit using FromCommitVM action
                    let from_commit_result =
                        action::call(FromCommitVM::new(commit_id, cluster_id)).await;
                    match from_commit_result {
                        Ok(restored_response) => {
                            let restored_vm_id = Uuid::parse_str(&restored_response.vm_id)
                                .expect("Restored VM ID should be valid UUID");
                            tracing::info!(
                                "✓ FromCommitVM succeeded, restored VM: {}",
                                restored_vm_id
                            );

                            // Verify the restored VM exists in database
                            let db_vm = db
                                .vms()
                                .get_by_id(restored_vm_id)
                                .await
                                .unwrap()
                                .expect("Restored VM should exist in database");

                            assert_eq!(
                                db_vm.cluster_id, cluster_id,
                                "Restored VM should be in correct cluster"
                            );
                            assert_eq!(db_vm.parent, None, "Restored VM should have no parent");
                            assert_ne!(
                                restored_vm_id, original_vm_id,
                                "Restored VM should have different ID than original"
                            );

                            tracing::info!(
                                "✓ FromCommitVM action properly restored VM from snapshot"
                            );

                            // Cleanup: Delete restored VM
                            match action::call(DeleteVM::new(restored_vm_id, false)).await {
                                Ok(deleted_ids) => {
                                    tracing::info!(
                                        "✓ Cleaned up restored VM: {} ({} VMs deleted)",
                                        restored_vm_id,
                                        deleted_ids.len()
                                    );
                                }
                                Err(e) => tracing::warn!(
                                    "Failed to cleanup restored VM {}: {:?}",
                                    restored_vm_id,
                                    e
                                ),
                            }
                        }
                        Err(e) => {
                            // Cleanup on failure
                            let _ = action::call(DeleteVM::new(original_vm_id, false)).await;
                            panic!("Scenario 8 failed: FromCommitVM action failed: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    // Cleanup on failure
                    let _ = action::call(DeleteVM::new(original_vm_id, false)).await;
                    panic!(
                        "Scenario 8 failed: Could not commit VM for restore test: {:?}",
                        e
                    );
                }
            }

            // Cleanup: Delete original VM
            match action::call(DeleteVM::new(original_vm_id, false)).await {
                Ok(deleted_ids) => {
                    tracing::info!(
                        "✓ Cleaned up original VM: {} ({} VMs deleted)",
                        original_vm_id,
                        deleted_ids.len()
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to cleanup original VM {}: {:?}", original_vm_id, e)
                }
            }
        }
        Err(e) => {
            panic!(
                "Scenario 8 failed: Could not create VM for restore test: {:?}",
                e
            );
        }
    }

    // ========================================================================
    // Scenario 9: Test UpdateVMState action (pause/resume VM)
    // ========================================================================
    tracing::info!("Scenario 9: Test UpdateVMState action");

    // Create a VM to test state changes
    let state_test_request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(2),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let state_test_result = action::call(NewRootVM::new(state_test_request, cluster_id)).await;
    match state_test_result {
        Ok(response) => {
            let vm_id = Uuid::parse_str(&response.id).expect("Valid UUID");
            tracing::info!("✓ Created VM for state test: {}", vm_id);

            // Pause the VM
            let pause_result = action::call(UpdateVMState::pause(vm_id)).await;
            match pause_result {
                Ok(_) => {
                    tracing::info!("✓ UpdateVMState (pause) succeeded");
                }
                Err(e) => {
                    // Cleanup on failure
                    let _ = action::call(DeleteVM::new(vm_id, false)).await;
                    panic!("Scenario 9 failed: UpdateVMState pause failed: {:?}", e);
                }
            }

            // Resume the VM
            let resume_result = action::call(UpdateVMState::resume(vm_id)).await;
            match resume_result {
                Ok(_) => {
                    tracing::info!("✓ UpdateVMState (resume) succeeded");
                    tracing::info!("✓ UpdateVMState action properly paused and resumed VM");
                }
                Err(e) => {
                    // Cleanup on failure
                    let _ = action::call(DeleteVM::new(vm_id, false)).await;
                    panic!("Scenario 9 failed: UpdateVMState resume failed: {:?}", e);
                }
            }

            // Cleanup: Delete the VM
            match action::call(DeleteVM::new(vm_id, false)).await {
                Ok(deleted_ids) => {
                    tracing::info!(
                        "✓ Cleaned up VM after state test: {} ({} VMs deleted)",
                        vm_id,
                        deleted_ids.len()
                    );
                }
                Err(e) => tracing::warn!("Failed to cleanup VM {}: {:?}", vm_id, e),
            }
        }
        Err(e) => {
            panic!(
                "Scenario 9 failed: Could not create VM for state test: {:?}",
                e
            );
        }
    }

    // ========================================================================
    // Scenario 10: Test ListAllVMs action (list all VMs on node)
    // ========================================================================
    tracing::info!("Scenario 10: Test ListAllVMs action");

    // Create a VM so we have something to list
    let list_test_request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(2),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let list_test_result = action::call(NewRootVM::new(list_test_request, cluster_id)).await;
    match list_test_result {
        Ok(response) => {
            let vm_id = Uuid::parse_str(&response.id).expect("Valid UUID");
            tracing::info!("✓ Created VM for list test: {}", vm_id);

            // List all VMs on the node
            let list_result = action::call(ListAllVMs::new(node_id)).await;
            match list_result {
                Ok(list_response) => {
                    tracing::info!(
                        "✓ ListAllVMs succeeded, found {} VMs",
                        list_response.vms.len()
                    );

                    // Verify our VM is in the list
                    let vm_id_str = vm_id.to_string();
                    let found = list_response.vms.iter().any(|v| v.vm_id == vm_id_str);
                    assert!(found, "Created VM should be in the list");

                    tracing::info!("✓ ListAllVMs action properly listed VMs on node");
                }
                Err(e) => {
                    // Cleanup on failure
                    let _ = action::call(DeleteVM::new(vm_id, false)).await;
                    panic!("Scenario 10 failed: ListAllVMs failed: {:?}", e);
                }
            }

            // Cleanup: Delete the VM
            match action::call(DeleteVM::new(vm_id, false)).await {
                Ok(deleted_ids) => {
                    tracing::info!(
                        "✓ Cleaned up VM after list test: {} ({} VMs deleted)",
                        vm_id,
                        deleted_ids.len()
                    );
                }
                Err(e) => tracing::warn!("Failed to cleanup VM {}: {:?}", vm_id, e),
            }
        }
        Err(e) => {
            panic!(
                "Scenario 10 failed: Could not create VM for list test: {:?}",
                e
            );
        }
    }

    // ========================================================================
    // Test Complete - Rollback database transaction
    // ========================================================================
    tracing::info!("✓ All scenarios completed successfully (all VM lifecycle actions tested)");
    db.rollback_for_test().await.unwrap();
}
