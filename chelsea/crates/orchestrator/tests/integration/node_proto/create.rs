use std::net::IpAddr;
use std::str::FromStr;

use dto_lib::chelsea_server2::vm::{VmCreateRequest, VmCreateVmConfig};
use orchestrator::outbound::node_proto::ChelseaProto;
use uuid::Uuid;

use crate::skip_if_no_endpoint;
use super::common::{create_test_vm, get_test_endpoint};

#[tokio::test]
async fn test_new_root_vm_success() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let vm_id = match create_test_vm(endpoint).await {
        Ok(vm_id) => {
            tracing::info!("Successfully created VM with ID: {}", vm_id);
            vm_id
        }
        Err(e) => {
            panic!("Expected successful VM creation, but got error: {}", e);
        }
    };

    // Cleanup: delete the VM
    let proto = ChelseaProto::new();
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_new_root_vm_minimal_config() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: None,
            image_name: None,
            vcpu_count: None,
            mem_size_mib: None,
            fs_size_mib: None,
        },
    };

    let result = proto.new_root_vm(endpoint, request).await;

    // This should either succeed with defaults or fail gracefully
    match result {
        Ok(response) => {
            let vm_id = Uuid::parse_str(&response.id).expect("Should be valid UUID");
            tracing::info!(
                "Successfully created VM with minimal config, ID: {}",
                response.id
            );

            // Cleanup: delete the VM
            match proto.delete_vm(endpoint, vm_id).await {
                Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
                Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
            }
        }
        Err(e) => {
            tracing::info!("Minimal config rejected as expected: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_new_root_vm_custom_resources() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(4),      // More CPUs
            mem_size_mib: Some(2048), // More memory
            fs_size_mib: Some(4096),  // Larger disk
        },
    };

    let result = proto.new_root_vm(endpoint, request).await;

    let vm_id = match result {
        Ok(response) => {
            let vm_id = Uuid::parse_str(&response.id).expect("Should be valid UUID");
            tracing::info!(
                "Successfully created VM with custom resources, ID: {}",
                response.id
            );
            vm_id
        }
        Err(e) => {
            panic!(
                "Expected successful VM creation with custom resources, but got error: {:?}",
                e
            );
        }
    };

    // Cleanup: delete the VM
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_new_root_vm_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1, should not be reachable

    let proto = ChelseaProto::new();
    let request = VmCreateRequest {
        vm_config: VmCreateVmConfig {
            kernel_name: Some("default.bin".to_string()),
            image_name: Some("default".to_string()),
            vcpu_count: Some(2),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let result = proto.new_root_vm(endpoint, request).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_new_root_vm_multiple_sequential() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let mut created_vms = Vec::new();

    // Create multiple VMs sequentially with small delay between creates
    for i in 0..3 {
        match create_test_vm(endpoint).await {
            Ok(vm_id) => {
                tracing::info!("Successfully created VM {}: {}", i, vm_id);
                created_vms.push(vm_id);
            }
            Err(e) => {
                panic!("VM {} creation failed: {}", i, e);
            }
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Cleanup: delete all created VMs
    for (i, vm_id) in created_vms.iter().enumerate() {
        match proto.delete_vm(endpoint, *vm_id).await {
            Ok(_) => tracing::info!("Cleaned up test VM {}: {}", i, vm_id),
            Err(e) => tracing::warn!("Failed to cleanup test VM {} ({}): {:?}", i, vm_id, e),
        }

        // Small delay to avoid overwhelming the server
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_new_root_vm_response_format() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let vm_id = match create_test_vm(endpoint).await {
        Ok(vm_id) => {
            // Verify it's a valid v4 UUID
            assert_eq!(
                vm_id.get_version(),
                Some(uuid::Version::Random),
                "Should be a v4 UUID"
            );

            tracing::info!("Response format validated for VM: {}", vm_id);
            vm_id
        }
        Err(e) => {
            panic!("Expected successful response to validate format: {}", e);
        }
    };

    // Cleanup: delete the VM
    let proto = ChelseaProto::new();
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}
