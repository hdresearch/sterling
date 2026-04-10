use std::net::IpAddr;
use std::str::FromStr;

use orchestrator::outbound::node_proto::ChelseaProto;
use uuid::Uuid;

use super::common::{create_test_vm, get_test_endpoint};
use crate::skip_if_no_endpoint;

#[tokio::test]
async fn test_vm_from_commit_success() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let original_vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create original VM");

    tracing::info!("Created original VM: {}", original_vm_id);

    let proto = ChelseaProto::new();

    // Commit the VM
    let commit_response = proto
        .vm_commit(endpoint, original_vm_id)
        .await
        .expect("Failed to commit VM");

    let commit_id = Uuid::parse_str(&commit_response.commit_id).expect("Should be valid UUID");
    tracing::info!("Committed VM to: {}", commit_id);

    // Restore from commit
    let result = proto.vm_from_commit(endpoint, commit_id).await;

    let restored_vm_id = match result {
        Ok(response) => {
            let vm_id =
                Uuid::parse_str(&response.vm_id).expect("Restored VM ID should be valid UUID");

            tracing::info!(
                "Successfully restored VM {} from commit {}",
                vm_id,
                commit_id
            );

            // Verify it's a valid v4 UUID
            assert_eq!(
                vm_id.get_version(),
                Some(uuid::Version::Random),
                "Restored VM ID should be a v4 UUID"
            );

            // Verify it's different from the original VM
            assert_ne!(
                vm_id, original_vm_id,
                "Restored VM should have different ID than original"
            );

            vm_id
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, original_vm_id).await;
            panic!(
                "Expected successful VM restore from commit, but got error: {:?}",
                e
            );
        }
    };

    // Cleanup: delete both VMs
    match proto.delete_vm(endpoint, restored_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up restored VM: {}", restored_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup restored VM {}: {:?}", restored_vm_id, e),
    }

    match proto.delete_vm(endpoint, original_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up original VM: {}", original_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup original VM {}: {:?}", original_vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_from_commit_nonexistent_commit() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Try to restore from a non-existent commit
    let fake_commit_id = Uuid::new_v4();

    let proto = ChelseaProto::new();
    let result = proto.vm_from_commit(endpoint, fake_commit_id).await;

    // Should fail - either with error or non-success status
    assert!(
        result.is_err(),
        "Expected error when restoring from non-existent commit"
    );
    tracing::info!(
        "Correctly failed to restore from non-existent commit: {:?}",
        result
    );
}

#[tokio::test]
async fn test_vm_from_commit_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let commit_id = Uuid::new_v4();
    let proto = ChelseaProto::new();
    let result = proto.vm_from_commit(endpoint, commit_id).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_vm_from_commit_multiple_restores() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let original_vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create original VM");

    tracing::info!("Created original VM: {}", original_vm_id);

    let proto = ChelseaProto::new();

    // Commit the VM
    let commit_response = proto
        .vm_commit(endpoint, original_vm_id)
        .await
        .expect("Failed to commit VM");

    let commit_id = Uuid::parse_str(&commit_response.commit_id).expect("Should be valid UUID");
    tracing::info!("Committed VM to: {}", commit_id);

    let mut restored_vms = Vec::new();

    // Restore from the same commit multiple times
    for i in 0..3 {
        match proto.vm_from_commit(endpoint, commit_id).await {
            Ok(response) => {
                let vm_id = Uuid::parse_str(&response.vm_id).expect("Should be valid UUID");
                tracing::info!("Restored VM {}: {}", i, vm_id);
                restored_vms.push(vm_id);
            }
            Err(e) => {
                // Cleanup before panicking
                let _ = proto.delete_vm(endpoint, original_vm_id).await;
                for vm in &restored_vms {
                    let _ = proto.delete_vm(endpoint, *vm).await;
                }
                panic!("Failed to restore VM {}: {:?}", i, e);
            }
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Verify all restored VM IDs are unique
    let unique_vms: std::collections::HashSet<_> = restored_vms.iter().collect();
    assert_eq!(
        unique_vms.len(),
        restored_vms.len(),
        "All restored VM IDs should be unique"
    );

    // Verify all are different from original
    for vm_id in &restored_vms {
        assert_ne!(
            *vm_id, original_vm_id,
            "Restored VMs should have different IDs than original"
        );
    }

    // Cleanup: delete all restored VMs
    for (i, vm_id) in restored_vms.iter().enumerate() {
        match proto.delete_vm(endpoint, *vm_id).await {
            Ok(_) => tracing::info!("Cleaned up restored VM {}: {}", i, vm_id),
            Err(e) => tracing::warn!("Failed to cleanup restored VM {} ({}): {:?}", i, vm_id, e),
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Cleanup: delete original VM
    match proto.delete_vm(endpoint, original_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up original VM: {}", original_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup original VM {}: {:?}", original_vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_from_commit_full_cycle() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create VM -> Commit -> Restore -> Commit again -> Restore again
    // This tests the complete lifecycle

    let proto = ChelseaProto::new();

    // Step 1: Create original VM
    let vm1_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create VM1");
    tracing::info!("Step 1: Created VM1: {}", vm1_id);

    // Step 2: Commit VM1
    let commit1 = proto
        .vm_commit(endpoint, vm1_id)
        .await
        .expect("Failed to commit VM1");
    let commit1_id = Uuid::parse_str(&commit1.commit_id).expect("Valid UUID");
    tracing::info!("Step 2: Committed VM1 to commit1: {}", commit1_id);

    // Step 3: Restore VM2 from commit1
    let vm2_response = proto
        .vm_from_commit(endpoint, commit1_id)
        .await
        .expect("Failed to restore VM2 from commit1");
    let vm2_id = Uuid::parse_str(&vm2_response.vm_id).expect("Valid UUID");
    tracing::info!("Step 3: Restored VM2 from commit1: {}", vm2_id);

    // Step 4: Commit VM2
    let commit2 = proto
        .vm_commit(endpoint, vm2_id)
        .await
        .expect("Failed to commit VM2");
    let commit2_id = Uuid::parse_str(&commit2.commit_id).expect("Valid UUID");
    tracing::info!("Step 4: Committed VM2 to commit2: {}", commit2_id);

    // Step 5: Restore VM3 from commit2
    let vm3_response = proto
        .vm_from_commit(endpoint, commit2_id)
        .await
        .expect("Failed to restore VM3 from commit2");
    let vm3_id = Uuid::parse_str(&vm3_response.vm_id).expect("Valid UUID");
    tracing::info!("Step 5: Restored VM3 from commit2: {}", vm3_id);

    // Verify all IDs are unique
    assert_ne!(vm1_id, vm2_id, "VM1 and VM2 should be different");
    assert_ne!(vm2_id, vm3_id, "VM2 and VM3 should be different");
    assert_ne!(vm1_id, vm3_id, "VM1 and VM3 should be different");
    assert_ne!(commit1_id, commit2_id, "Commits should be different");

    tracing::info!("Full cycle completed successfully!");

    // Cleanup: delete all VMs
    for (name, vm_id) in [("VM3", vm3_id), ("VM2", vm2_id), ("VM1", vm1_id)] {
        match proto.delete_vm(endpoint, vm_id).await {
            Ok(_) => tracing::info!("Cleaned up {}: {}", name, vm_id),
            Err(e) => tracing::warn!("Failed to cleanup {} ({}): {:?}", name, vm_id, e),
        }
    }
}

#[tokio::test]
async fn test_vm_from_commit_branch_commit_restore() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Test: Create parent -> Branch child -> Commit child -> Restore from child commit
    let proto = ChelseaProto::new();

    // Create parent VM
    let parent_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");
    tracing::info!("Created parent VM: {}", parent_id);

    // Branch from parent
    let child_response = proto
        .vm_branch(endpoint, parent_id)
        .await
        .expect("Failed to branch child VM");
    let child_id = Uuid::parse_str(&child_response.vm_id).expect("Valid UUID");
    tracing::info!("Branched child VM: {}", child_id);

    // Commit the child
    let commit_response = proto
        .vm_commit(endpoint, child_id)
        .await
        .expect("Failed to commit child VM");
    let commit_id = Uuid::parse_str(&commit_response.commit_id).expect("Valid UUID");
    tracing::info!("Committed child VM to: {}", commit_id);

    // Restore from the commit
    let restored_response = proto
        .vm_from_commit(endpoint, commit_id)
        .await
        .expect("Failed to restore from commit");
    let restored_id = Uuid::parse_str(&restored_response.vm_id).expect("Valid UUID");
    tracing::info!("Restored VM from commit: {}", restored_id);

    // Verify all IDs are unique
    assert_ne!(parent_id, child_id, "Parent and child should be different");
    assert_ne!(
        child_id, restored_id,
        "Child and restored should be different"
    );
    assert_ne!(
        parent_id, restored_id,
        "Parent and restored should be different"
    );

    // Cleanup: delete in order (leaves first)
    for (name, vm_id) in [
        ("Restored", restored_id),
        ("Child", child_id),
        ("Parent", parent_id),
    ] {
        match proto.delete_vm(endpoint, vm_id).await {
            Ok(_) => tracing::info!("Cleaned up {} VM: {}", name, vm_id),
            Err(e) => tracing::warn!("Failed to cleanup {} VM ({}): {:?}", name, vm_id, e),
        }
    }
}

#[tokio::test]
async fn test_vm_from_commit_response_format() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM and commit it
    let original_vm_id = create_test_vm(endpoint).await.expect("Failed to create VM");

    let proto = ChelseaProto::new();

    let commit_response = proto
        .vm_commit(endpoint, original_vm_id)
        .await
        .expect("Failed to commit VM");

    let commit_id = Uuid::parse_str(&commit_response.commit_id).expect("Valid UUID");

    // Restore and verify response format
    let result = proto.vm_from_commit(endpoint, commit_id).await;

    let restored_vm_id = match result {
        Ok(response) => {
            // Verify the response contains a valid VM ID (UUID)
            let vm_id = Uuid::parse_str(&response.vm_id)
                .expect("Response should contain valid UUID for vm_id");

            // Verify it's a v4 UUID
            assert_eq!(
                vm_id.get_version(),
                Some(uuid::Version::Random),
                "VM ID should be v4 UUID"
            );

            tracing::info!("Response format validated: {:?}", response);
            vm_id
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, original_vm_id).await;
            panic!("Expected successful response to validate format: {:?}", e);
        }
    };

    // Cleanup
    match proto.delete_vm(endpoint, restored_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up restored VM: {}", restored_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup restored VM {}: {:?}", restored_vm_id, e),
    }

    match proto.delete_vm(endpoint, original_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up original VM: {}", original_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup original VM {}: {:?}", original_vm_id, e),
    }
}
