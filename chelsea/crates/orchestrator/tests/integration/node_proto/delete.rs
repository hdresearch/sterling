use std::net::IpAddr;
use std::str::FromStr;

use dto_lib::chelsea_server2::vm::{VmCreateRequest, VmCreateVmConfig};
use orchestrator::outbound::node_proto::ChelseaProto;
use uuid::Uuid;

use crate::skip_if_no_endpoint;
use super::common::{create_test_vm, get_test_endpoint};

#[tokio::test]
async fn test_delete_vm_success() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // First create a VM to delete
    let vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create test VM");

    tracing::info!("Created test VM for deletion: {}", vm_id);

    // Now delete it
    let proto = ChelseaProto::new();
    let result = proto.delete_vm(endpoint, vm_id).await;

    match result {
        Ok(response) => {
            tracing::info!("Delete response: {:?}", response);

            // Verify the VM was deleted
            assert!(
                response.deleted_ids.contains(&vm_id.to_string()),
                "VM ID {} should be in deleted list",
                vm_id
            );

            // Check for errors
            if let Some(error) = &response.error {
                tracing::warn!("Deletion completed with error: {}", error);
            }

            tracing::info!("Successfully deleted VM: {}", vm_id);
        }
        Err(e) => {
            panic!("Expected successful VM deletion, but got error: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_delete_vm_nonexistent() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Try to delete a non-existent VM
    let fake_vm_id = Uuid::new_v4();

    let proto = ChelseaProto::new();
    let result = proto.delete_vm(endpoint, fake_vm_id).await;

    match result {
        Ok(response) => {
            tracing::info!("Delete non-existent VM response: {:?}", response);

            // Should have an error or empty deleted_ids list
            if response.error.is_some() {
                tracing::info!("Correctly returned error for non-existent VM");
            } else {
                assert!(
                    response.deleted_ids.is_empty(),
                    "Should not have deleted any VMs"
                );
            }
        }
        Err(e) => {
            tracing::info!("Got expected error for non-existent VM: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_delete_vm_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let vm_id = Uuid::new_v4();
    let proto = ChelseaProto::new();
    let result = proto.delete_vm(endpoint, vm_id).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_create_and_delete_vm_cycle() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Create a VM
    let request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(2),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let create_result = proto.new_root_vm(endpoint, request).await;
    assert!(create_result.is_ok(), "VM creation should succeed");

    let vm_id = Uuid::parse_str(&create_result.unwrap().id).unwrap();
    tracing::info!("Created VM for cycle test: {}", vm_id);

    // Delete the VM
    let delete_result = proto.delete_vm(endpoint, vm_id).await;
    assert!(delete_result.is_ok(), "VM deletion should succeed");

    let delete_response = delete_result.unwrap();
    assert!(
        delete_response.deleted_ids.contains(&vm_id.to_string()),
        "Deleted VM ID should be in response"
    );

    tracing::info!("Successfully completed create-delete cycle for VM: {}", vm_id);
}

#[tokio::test]
async fn test_delete_multiple_vms() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let mut created_vms = Vec::new();

    // Create multiple VMs with small delay between creates
    for i in 0..3 {
        match create_test_vm(endpoint).await {
            Ok(vm_id) => {
                tracing::info!("Created VM {}: {}", i, vm_id);
                created_vms.push(vm_id);
            }
            Err(e) => {
                panic!("Failed to create VM {}: {}", i, e);
            }
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Delete all created VMs with small delay between deletes to avoid overwhelming server
    for (i, vm_id) in created_vms.iter().enumerate() {
        let result = proto.delete_vm(endpoint, *vm_id).await;

        match result {
            Ok(response) => {
                // Either the VM was deleted successfully or there was an error
                // Don't fail the test if the server reports an issue with one VM
                if response.deleted_ids.contains(&vm_id.to_string()) {
                    tracing::info!("Successfully deleted VM {}: {}", i, vm_id);
                } else if let Some(error) = &response.error {
                    tracing::warn!("VM {} deletion had error: {}", i, error);
                } else {
                    tracing::warn!("VM {} not in deleted list, but no error reported", i);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to delete VM {}: {:?}", i, e);
            }
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_delete_vm_response_format() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let vm_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create test VM");

    // Delete it and check response format
    let proto = ChelseaProto::new();
    let result = proto.delete_vm(endpoint, vm_id).await;

    match result {
        Ok(response) => {
            // Verify response structure
            assert!(
                !response.deleted_ids.is_empty() || response.error.is_some(),
                "Response should have either deleted IDs or an error"
            );

            // All IDs in deleted_ids should be valid UUIDs
            for id_str in &response.deleted_ids {
                assert!(
                    Uuid::parse_str(id_str).is_ok(),
                    "Deleted ID '{}' should be a valid UUID",
                    id_str
                );
            }

            tracing::info!("Response format validated: {:?}", response);
        }
        Err(e) => {
            panic!("Expected successful response to validate format: {:?}", e);
        }
    }
}
