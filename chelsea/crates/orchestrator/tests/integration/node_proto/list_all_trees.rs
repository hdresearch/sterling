use std::net::IpAddr;
use std::str::FromStr;

use orchestrator::outbound::node_proto::ChelseaProto;
use uuid::Uuid;

use super::common::{create_test_vm, get_test_endpoint};
use crate::skip_if_no_endpoint;

#[tokio::test]
async fn test_vm_list_all_empty() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let result = proto.vm_list_all(endpoint).await;

    match result {
        Ok(response) => {
            tracing::info!("VM list response: {:?}", response);
            tracing::info!("Found {} VMs", response.vms.len());
            // Could be empty or have existing VMs - both are valid
        }
        Err(e) => {
            panic!("Expected successful VM list, but got error: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_vm_list_all_with_single_vm() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Get initial count
    let initial_response = proto
        .vm_list_all(endpoint)
        .await
        .expect("Failed to get initial VM list");
    let initial_count = initial_response.vms.len();
    tracing::info!("Initial VM count: {}", initial_count);

    // Create a VM
    let vm_id = create_test_vm(endpoint).await.expect("Failed to create VM");
    tracing::info!("Created VM: {}", vm_id);

    // List VMs again
    let result = proto.vm_list_all(endpoint).await;

    match result {
        Ok(response) => {
            tracing::info!("Found {} VMs after creation", response.vms.len());

            // Should have at least one more VM than before
            assert!(
                response.vms.len() >= initial_count + 1,
                "Expected at least {} VMs, found {}",
                initial_count + 1,
                response.vms.len()
            );

            // Find our created VM in the list
            let found = response.vms.iter().find(|vm| vm.vm_id == vm_id.to_string());
            assert!(
                found.is_some(),
                "Created VM {} should be in the list",
                vm_id
            );

            if let Some(vm) = found {
                tracing::info!("Found created VM: {:?}", vm);
                // Root VMs should have no parent
                assert!(vm.parent_id.is_none(), "Root VM should have no parent");
            }
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, vm_id).await;
            panic!("Expected successful VM list, but got error: {:?}", e);
        }
    }

    // Cleanup
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_list_all_with_branch() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Create parent VM
    let parent_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");
    tracing::info!("Created parent VM: {}", parent_id);

    // Branch from parent
    let branch_response = proto
        .vm_branch(endpoint, parent_id)
        .await
        .expect("Failed to branch VM");
    let child_id = Uuid::parse_str(&branch_response.vm_id).expect("Valid UUID");
    tracing::info!("Created child VM: {}", child_id);

    // List VMs
    let result = proto.vm_list_all(endpoint).await;

    match result {
        Ok(response) => {
            tracing::info!("Found {} VMs", response.vms.len());

            // Find parent in list
            let found_parent = response
                .vms
                .iter()
                .find(|vm| vm.vm_id == parent_id.to_string());
            assert!(
                found_parent.is_some(),
                "Parent VM {} should be in the list",
                parent_id
            );

            if let Some(parent) = found_parent {
                tracing::info!("Found parent VM: {:?}", parent);
                assert!(
                    parent.parent_id.is_none(),
                    "Parent VM should have no parent"
                );
            }

            // Find child in list
            let found_child = response
                .vms
                .iter()
                .find(|vm| vm.vm_id == child_id.to_string());
            assert!(
                found_child.is_some(),
                "Child VM {} should be in the list",
                child_id
            );

            if let Some(child) = found_child {
                tracing::info!("Found child VM: {:?}", child);
                // Child should have parent_id set
                assert!(child.parent_id.is_some(), "Child VM should have a parent");
                assert_eq!(
                    child.parent_id.as_ref().unwrap(),
                    &parent_id.to_string(),
                    "Child's parent_id should match parent VM"
                );
            }
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, child_id).await;
            let _ = proto.delete_vm(endpoint, parent_id).await;
            panic!("Expected successful VM list, but got error: {:?}", e);
        }
    }

    // Cleanup: delete child first, then parent
    match proto.delete_vm(endpoint, child_id).await {
        Ok(_) => tracing::info!("Cleaned up child VM: {}", child_id),
        Err(e) => tracing::warn!("Failed to cleanup child VM {}: {:?}", child_id, e),
    }

    match proto.delete_vm(endpoint, parent_id).await {
        Ok(_) => tracing::info!("Cleaned up parent VM: {}", parent_id),
        Err(e) => tracing::warn!("Failed to cleanup parent VM {}: {:?}", parent_id, e),
    }
}

#[tokio::test]
async fn test_vm_list_all_multiple_roots() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Get initial count
    let initial_response = proto
        .vm_list_all(endpoint)
        .await
        .expect("Failed to get initial VM list");
    let initial_count = initial_response.vms.len();

    // Create multiple root VMs
    let mut root_vms = Vec::new();
    for i in 0..3 {
        match create_test_vm(endpoint).await {
            Ok(vm_id) => {
                tracing::info!("Created root VM {}: {}", i, vm_id);
                root_vms.push(vm_id);
            }
            Err(e) => {
                // Cleanup before panicking
                for vm in &root_vms {
                    let _ = proto.delete_vm(endpoint, *vm).await;
                }
                panic!("Failed to create root VM {}: {}", i, e);
            }
        }

        // Small delay
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // List VMs
    let result = proto.vm_list_all(endpoint).await;

    match result {
        Ok(response) => {
            tracing::info!("Found {} total VMs", response.vms.len());

            // Should have at least 3 more VMs than initially
            assert!(
                response.vms.len() >= initial_count + 3,
                "Expected at least {} VMs, found {}",
                initial_count + 3,
                response.vms.len()
            );

            // All our created VMs should be in the list and be root VMs
            for (i, vm_id) in root_vms.iter().enumerate() {
                let found = response.vms.iter().find(|vm| vm.vm_id == vm_id.to_string());
                assert!(
                    found.is_some(),
                    "Root VM {} ({}) should be in the list",
                    i,
                    vm_id
                );

                if let Some(vm) = found {
                    assert!(
                        vm.parent_id.is_none(),
                        "Root VM {} should have no parent",
                        i
                    );
                }
            }
        }
        Err(e) => {
            // Cleanup before panicking
            for vm in &root_vms {
                let _ = proto.delete_vm(endpoint, *vm).await;
            }
            panic!("Expected successful VM list, but got error: {:?}", e);
        }
    }

    // Cleanup: delete all root VMs
    for (i, vm_id) in root_vms.iter().enumerate() {
        match proto.delete_vm(endpoint, *vm_id).await {
            Ok(_) => tracing::info!("Cleaned up root VM {}: {}", i, vm_id),
            Err(e) => tracing::warn!("Failed to cleanup root VM {} ({}): {:?}", i, vm_id, e),
        }

        // Small delay
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_vm_list_all_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let proto = ChelseaProto::new();
    let result = proto.vm_list_all(endpoint).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_vm_list_all_response_format() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Create a parent and child to ensure we have data
    let parent_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");

    let branch_response = proto
        .vm_branch(endpoint, parent_id)
        .await
        .expect("Failed to branch VM");
    let child_id = Uuid::parse_str(&branch_response.vm_id).expect("Valid UUID");

    // List VMs and validate format
    let result = proto.vm_list_all(endpoint).await;

    match result {
        Ok(response) => {
            // Response should have vms field
            tracing::info!("Response validated: {} VMs found", response.vms.len());

            // Each VM should have valid fields
            for vm in &response.vms {
                // vm_id should be a valid UUID
                let _parsed_id = Uuid::parse_str(&vm.vm_id).expect("vm_id should be valid UUID");

                // If parent_id is present, it should also be valid UUID
                if let Some(ref parent_id_str) = vm.parent_id {
                    let _parsed_parent =
                        Uuid::parse_str(parent_id_str).expect("parent_id should be valid UUID");
                }

                tracing::info!("VM validated: {:?}", vm);
            }
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, child_id).await;
            let _ = proto.delete_vm(endpoint, parent_id).await;
            panic!("Expected successful response to validate format: {:?}", e);
        }
    }

    // Cleanup
    let _ = proto.delete_vm(endpoint, child_id).await;
    let _ = proto.delete_vm(endpoint, parent_id).await;
}

#[tokio::test]
async fn test_vm_list_all_after_delete() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Get initial count
    let initial_response = proto
        .vm_list_all(endpoint)
        .await
        .expect("Failed to get initial VM list");
    let initial_count = initial_response.vms.len();

    // Try to create a VM - if server is at capacity, skip this test
    let vm_id = match create_test_vm(endpoint).await {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!("Could not create VM (server may be at capacity): {:?}", e);
            tracing::info!("Skipping test due to inability to create VM");
            return;
        }
    };
    tracing::info!("Created VM: {}", vm_id);

    // Verify it's in the list
    let after_create = proto
        .vm_list_all(endpoint)
        .await
        .expect("Failed to get VM list after create");
    assert!(
        after_create
            .vms
            .iter()
            .any(|vm| vm.vm_id == vm_id.to_string()),
        "VM should be in list after creation"
    );

    // Delete the VM
    proto
        .delete_vm(endpoint, vm_id)
        .await
        .expect("Failed to delete VM");
    tracing::info!("Deleted VM: {}", vm_id);

    // Verify it's no longer in the list
    let after_delete = proto
        .vm_list_all(endpoint)
        .await
        .expect("Failed to get VM list after delete");

    let still_exists = after_delete
        .vms
        .iter()
        .any(|vm| vm.vm_id == vm_id.to_string());
    assert!(!still_exists, "VM should not be in list after deletion");

    // Count should be back to initial (or close to it)
    tracing::info!(
        "VM count: initial={}, after_delete={}",
        initial_count,
        after_delete.vms.len()
    );
}
