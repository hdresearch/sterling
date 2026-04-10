use std::net::IpAddr;
use std::str::FromStr;

use orchestrator::outbound::node_proto::ChelseaProto;
use uuid::Uuid;

use crate::skip_if_no_endpoint;
use super::common::{create_test_vm, get_test_endpoint};

#[tokio::test]
async fn test_vm_branch_success() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // First create a parent VM
    let parent_vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");

    tracing::info!("Created parent VM for branching: {}", parent_vm_id);

    // Now branch from it
    let proto = ChelseaProto::new();
    let result = proto.vm_branch(endpoint, parent_vm_id).await;

    let child_vm_id = match result {
        Ok(response) => {
            let child_id = Uuid::parse_str(&response.vm_id)
                .expect("Child VM ID should be valid UUID");

            tracing::info!(
                "Successfully branched VM {} from parent {}",
                child_id,
                parent_vm_id
            );

            // Verify it's a valid v4 UUID
            assert_eq!(
                child_id.get_version(),
                Some(uuid::Version::Random),
                "Should be a v4 UUID"
            );

            child_id
        }
        Err(e) => {
            panic!("Expected successful VM branch, but got error: {:?}", e);
        }
    };

    // Cleanup: delete both VMs (child first, then parent)
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
async fn test_vm_branch_nonexistent_parent() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Try to branch from a non-existent VM
    let fake_vm_id = Uuid::new_v4();

    let proto = ChelseaProto::new();
    let result = proto.vm_branch(endpoint, fake_vm_id).await;

    // Should fail - either with error or non-success status
    assert!(
        result.is_err(),
        "Expected error when branching from non-existent VM"
    );
    tracing::info!("Correctly failed to branch from non-existent VM: {:?}", result);
}

#[tokio::test]
async fn test_vm_branch_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let vm_id = Uuid::new_v4();
    let proto = ChelseaProto::new();
    let result = proto.vm_branch(endpoint, vm_id).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_vm_branch_multiple_children() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a parent VM
    let parent_vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");

    tracing::info!("Created parent VM: {}", parent_vm_id);

    let proto = ChelseaProto::new();
    let mut child_vms = Vec::new();

    // Create multiple branches from the same parent
    for i in 0..3 {
        match proto.vm_branch(endpoint, parent_vm_id).await {
            Ok(response) => {
                let child_id = Uuid::parse_str(&response.vm_id)
                    .expect("Should be valid UUID");
                tracing::info!("Created child VM {}: {}", i, child_id);
                child_vms.push(child_id);
            }
            Err(e) => {
                panic!("Failed to create child VM {}: {:?}", i, e);
            }
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Cleanup: delete all child VMs first, then parent
    for (i, child_id) in child_vms.iter().enumerate() {
        match proto.delete_vm(endpoint, *child_id).await {
            Ok(_) => tracing::info!("Cleaned up child VM {}: {}", i, child_id),
            Err(e) => tracing::warn!("Failed to cleanup child VM {} ({}): {:?}", i, child_id, e),
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    match proto.delete_vm(endpoint, parent_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up parent VM: {}", parent_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup parent VM {}: {:?}", parent_vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_branch_nested_hierarchy() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a root VM
    let root_vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create root VM");

    tracing::info!("Created root VM: {}", root_vm_id);

    let proto = ChelseaProto::new();

    // Create first generation child
    let gen1_response = proto.vm_branch(endpoint, root_vm_id)
        .await
        .expect("Failed to create gen1 VM");
    let gen1_vm_id = Uuid::parse_str(&gen1_response.vm_id)
        .expect("Should be valid UUID");
    tracing::info!("Created gen1 VM: {}", gen1_vm_id);

    // Create second generation child (grandchild of root)
    let gen2_response = proto.vm_branch(endpoint, gen1_vm_id)
        .await
        .expect("Failed to create gen2 VM");
    let gen2_vm_id = Uuid::parse_str(&gen2_response.vm_id)
        .expect("Should be valid UUID");
    tracing::info!("Created gen2 VM: {}", gen2_vm_id);

    // Cleanup: delete in reverse order (leaf to root)
    match proto.delete_vm(endpoint, gen2_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up gen2 VM: {}", gen2_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup gen2 VM {}: {:?}", gen2_vm_id, e),
    }

    match proto.delete_vm(endpoint, gen1_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up gen1 VM: {}", gen1_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup gen1 VM {}: {:?}", gen1_vm_id, e),
    }

    match proto.delete_vm(endpoint, root_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up root VM: {}", root_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup root VM {}: {:?}", root_vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_branch_response_format() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a parent VM
    let parent_vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");

    // Branch from it and verify response format
    let proto = ChelseaProto::new();
    let result = proto.vm_branch(endpoint, parent_vm_id).await;

    let child_vm_id = match result {
        Ok(response) => {
            // Verify the response contains a valid UUID
            let child_id = Uuid::parse_str(&response.vm_id)
                .expect("Response should contain valid UUID");

            // Verify it's different from parent
            assert_ne!(
                child_id,
                parent_vm_id,
                "Child VM ID should be different from parent"
            );

            tracing::info!("Response format validated: {:?}", response);
            child_id
        }
        Err(e) => {
            panic!("Expected successful response to validate format: {:?}", e);
        }
    };

    // Cleanup
    match proto.delete_vm(endpoint, child_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up child VM: {}", child_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup child VM {}: {:?}", child_vm_id, e),
    }

    match proto.delete_vm(endpoint, parent_vm_id).await {
        Ok(_) => tracing::info!("Cleaned up parent VM: {}", parent_vm_id),
        Err(e) => tracing::warn!("Failed to cleanup parent VM {}: {:?}", parent_vm_id, e),
    }
}
