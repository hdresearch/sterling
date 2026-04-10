use std::net::IpAddr;
use std::str::FromStr;

use orchestrator::outbound::node_proto::ChelseaProto;
use uuid::Uuid;

use crate::skip_if_no_endpoint;
use super::common::{create_test_vm, get_test_endpoint};

#[tokio::test]
async fn test_vm_commit_success() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // First create a VM to commit
    let vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create VM");

    tracing::info!("Created VM for commit test: {}", vm_id);

    // Now commit the VM
    let proto = ChelseaProto::new();
    let result = proto.vm_commit(endpoint, vm_id).await;

    match result {
        Ok(response) => {
            // Verify we got a valid commit ID
            let commit_id = Uuid::parse_str(&response.commit_id)
                .expect("Commit ID should be valid UUID");

            tracing::info!(
                "Successfully committed VM {} to commit {}",
                vm_id,
                commit_id
            );

            // Verify it's a valid v4 UUID
            assert_eq!(
                commit_id.get_version(),
                Some(uuid::Version::Random),
                "Commit ID should be a v4 UUID"
            );

            // Verify we got host architecture
            assert!(
                !response.host_architecture.is_empty(),
                "Host architecture should not be empty"
            );
            tracing::info!("Host architecture: {}", response.host_architecture);
        }
        Err(e) => {
            panic!("Expected successful VM commit, but got error: {:?}", e);
        }
    }

    // Cleanup: delete the VM
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_commit_nonexistent_vm() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Try to commit a non-existent VM
    let fake_vm_id = Uuid::new_v4();

    let proto = ChelseaProto::new();
    let result = proto.vm_commit(endpoint, fake_vm_id).await;

    // Should fail - either with error or non-success status
    assert!(
        result.is_err(),
        "Expected error when committing non-existent VM"
    );
    tracing::info!("Correctly failed to commit non-existent VM: {:?}", result);
}

#[tokio::test]
async fn test_vm_commit_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let vm_id = Uuid::new_v4();
    let proto = ChelseaProto::new();
    let result = proto.vm_commit(endpoint, vm_id).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_vm_commit_multiple_sequential() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create VM");

    tracing::info!("Created VM for multiple commit test: {}", vm_id);

    let proto = ChelseaProto::new();
    let mut commit_ids = Vec::new();

    // Create multiple commits from the same VM
    for i in 0..3 {
        match proto.vm_commit(endpoint, vm_id).await {
            Ok(response) => {
                let commit_id = Uuid::parse_str(&response.commit_id)
                    .expect("Should be valid UUID");
                tracing::info!("Created commit {}: {}", i, commit_id);
                commit_ids.push(commit_id);
            }
            Err(e) => {
                // Cleanup before panicking
                let _ = proto.delete_vm(endpoint, vm_id).await;
                panic!("Failed to create commit {}: {:?}", i, e);
            }
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Verify all commit IDs are unique
    let unique_commits: std::collections::HashSet<_> = commit_ids.iter().collect();
    assert_eq!(
        unique_commits.len(),
        commit_ids.len(),
        "All commit IDs should be unique"
    );

    // Cleanup: delete the VM
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_commit_branch_and_commit() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a parent VM
    let parent_vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");

    tracing::info!("Created parent VM: {}", parent_vm_id);

    let proto = ChelseaProto::new();

    // Commit the parent
    let parent_commit = proto.vm_commit(endpoint, parent_vm_id)
        .await
        .expect("Failed to commit parent VM");

    let parent_commit_id = Uuid::parse_str(&parent_commit.commit_id)
        .expect("Should be valid UUID");
    tracing::info!("Committed parent VM to: {}", parent_commit_id);

    // Branch from parent
    let branch_response = proto.vm_branch(endpoint, parent_vm_id)
        .await
        .expect("Failed to branch VM");

    let child_vm_id = Uuid::parse_str(&branch_response.vm_id)
        .expect("Should be valid UUID");
    tracing::info!("Created child VM: {}", child_vm_id);

    // Commit the child
    let child_commit = proto.vm_commit(endpoint, child_vm_id)
        .await
        .expect("Failed to commit child VM");

    let child_commit_id = Uuid::parse_str(&child_commit.commit_id)
        .expect("Should be valid UUID");
    tracing::info!("Committed child VM to: {}", child_commit_id);

    // Verify commit IDs are different
    assert_ne!(
        parent_commit_id,
        child_commit_id,
        "Parent and child commits should have different IDs"
    );

    // Cleanup: delete child first, then parent
    match proto.delete_vm(endpoint, child_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up child VM: {}", child_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup child VM {}: {:?}", child_vm_id, e),
    }

    match proto.delete_vm(endpoint, parent_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up parent VM: {}", parent_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup parent VM {}: {:?}", parent_vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_commit_response_format() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create VM");

    // Commit it and verify response format
    let proto = ChelseaProto::new();
    let result = proto.vm_commit(endpoint, vm_id).await;

    match result {
        Ok(response) => {
            // Verify the response contains a valid commit ID (UUID)
            let _commit_id = Uuid::parse_str(&response.commit_id)
                .expect("Response should contain valid UUID for commit_id");

            // Verify host architecture is present and non-empty
            assert!(
                !response.host_architecture.is_empty(),
                "host_architecture should not be empty"
            );

            // Common architectures: x86_64, aarch64, etc.
            tracing::info!("Response format validated: {:?}", response);
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, vm_id).await;
            panic!("Expected successful response to validate format: {:?}", e);
        }
    }

    // Cleanup
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}
