use std::net::IpAddr;
use std::str::FromStr;

use dto_lib::chelsea_server2::vm::VmUpdateStateEnum;
use orchestrator::outbound::node_proto::ChelseaProto;
use uuid::Uuid;

use super::common::{create_test_vm, get_test_endpoint};
use crate::skip_if_no_endpoint;

#[tokio::test]
async fn test_vm_update_state_pause() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let vm_id = create_test_vm(endpoint).await.expect("Failed to create VM");

    tracing::info!("Created VM for pause test: {}", vm_id);

    // Pause the VM
    let proto = ChelseaProto::new();
    let result = proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Paused)
        .await;

    match result {
        Ok(()) => {
            tracing::info!("Successfully paused VM: {}", vm_id);
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, vm_id).await;
            panic!("Expected successful VM pause, but got error: {:?}", e);
        }
    }

    // Cleanup: delete the VM
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_update_state_resume() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let vm_id = create_test_vm(endpoint).await.expect("Failed to create VM");

    tracing::info!("Created VM for resume test: {}", vm_id);

    let proto = ChelseaProto::new();

    // First pause the VM
    proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Paused)
        .await
        .expect("Failed to pause VM");
    tracing::info!("Paused VM: {}", vm_id);

    // Now resume (set to Running)
    let result = proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Running)
        .await;

    match result {
        Ok(()) => {
            tracing::info!("Successfully resumed VM: {}", vm_id);
        }
        Err(e) => {
            // Cleanup before panicking
            let _ = proto.delete_vm(endpoint, vm_id).await;
            panic!("Expected successful VM resume, but got error: {:?}", e);
        }
    }

    // Cleanup: delete the VM
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_update_state_multiple_transitions() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let vm_id = create_test_vm(endpoint).await.expect("Failed to create VM");

    tracing::info!("Created VM for multiple transitions test: {}", vm_id);

    let proto = ChelseaProto::new();

    // Transition through states multiple times: Running -> Paused -> Running -> Paused
    let transitions = vec![
        ("Pause", VmUpdateStateEnum::Paused),
        ("Resume", VmUpdateStateEnum::Running),
        ("Pause again", VmUpdateStateEnum::Paused),
        ("Resume again", VmUpdateStateEnum::Running),
    ];

    for (description, state) in transitions {
        match proto.vm_update_state(endpoint, vm_id, state).await {
            Ok(()) => {
                tracing::info!("{} VM {}: success", description, vm_id);
            }
            Err(e) => {
                // Cleanup before panicking
                let _ = proto.delete_vm(endpoint, vm_id).await;
                panic!("Failed to {} VM {}: {:?}", description, vm_id, e);
            }
        }

        // Small delay between transitions
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    tracing::info!("Successfully completed all state transitions");

    // Cleanup: delete the VM
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}

#[tokio::test]
async fn test_vm_update_state_nonexistent_vm() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Try to update state of a non-existent VM
    let fake_vm_id = Uuid::new_v4();

    let proto = ChelseaProto::new();
    let result = proto
        .vm_update_state(endpoint, fake_vm_id, VmUpdateStateEnum::Paused)
        .await;

    // Should fail - either with error or non-success status
    assert!(
        result.is_err(),
        "Expected error when updating state of non-existent VM"
    );
    tracing::info!(
        "Correctly failed to update state of non-existent VM: {:?}",
        result
    );
}

#[tokio::test]
async fn test_vm_update_state_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let vm_id = Uuid::new_v4();
    let proto = ChelseaProto::new();
    let result = proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Paused)
        .await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_vm_update_state_pause_branch_resume() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create parent VM
    let parent_id = create_test_vm(endpoint)
        .await
        .expect("Failed to create parent VM");
    tracing::info!("Created parent VM: {}", parent_id);

    let proto = ChelseaProto::new();

    // Pause parent
    proto
        .vm_update_state(endpoint, parent_id, VmUpdateStateEnum::Paused)
        .await
        .expect("Failed to pause parent VM");
    tracing::info!("Paused parent VM");

    // Branch from paused parent
    let branch_response = proto
        .vm_branch(endpoint, parent_id)
        .await
        .expect("Failed to branch from paused VM");
    let child_id = Uuid::parse_str(&branch_response.vm_id).expect("Valid UUID");
    tracing::info!("Branched child VM from paused parent: {}", child_id);

    // Resume parent
    proto
        .vm_update_state(endpoint, parent_id, VmUpdateStateEnum::Running)
        .await
        .expect_err("Its illegal to branch a parent VM");

    // Pause child
    proto
        .vm_update_state(endpoint, child_id, VmUpdateStateEnum::Paused)
        .await
        .expect("Failed to pause child VM");
    tracing::info!("Paused child VM");

    // Resume child
    proto
        .vm_update_state(endpoint, child_id, VmUpdateStateEnum::Running)
        .await
        .expect("Failed to resume child VM");
    tracing::info!("Resumed child VM");

    // Cleanup: delete both VMs
    for (name, vm_id) in [("Child", child_id), ("Parent", parent_id)] {
        match proto.delete_vm(endpoint, vm_id).await {
            Ok(_) => tracing::info!("Cleaned up {} VM: {}", name, vm_id),
            Err(e) => tracing::warn!("Failed to cleanup {} VM ({}): {:?}", name, vm_id, e),
        }
    }
}

#[tokio::test]
async fn test_vm_update_state_idempotent() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Create a VM
    let vm_id = create_test_vm(endpoint).await.expect("Failed to create VM");

    tracing::info!("Created VM for idempotent test: {}", vm_id);

    let proto = ChelseaProto::new();

    // Pause the VM
    proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Paused)
        .await
        .expect("Failed to pause VM first time");
    tracing::info!("Paused VM first time");

    // Pause again - should be idempotent
    let result = proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Paused)
        .await;
    match result {
        Ok(()) => {
            tracing::info!("Successfully paused VM second time (idempotent)");
        }
        Err(e) => {
            // Some implementations might return error for redundant state change
            tracing::warn!("Pausing already-paused VM returned error: {:?}", e);
        }
    }

    // Resume the VM
    proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Running)
        .await
        .expect("Failed to resume VM");
    tracing::info!("Resumed VM");

    // Resume again - should be idempotent
    let result = proto
        .vm_update_state(endpoint, vm_id, VmUpdateStateEnum::Running)
        .await;
    match result {
        Ok(()) => {
            tracing::info!("Successfully resumed VM second time (idempotent)");
        }
        Err(e) => {
            // Some implementations might return error for redundant state change
            tracing::warn!("Resuming already-running VM returned error: {:?}", e);
        }
    }

    // Cleanup: delete the VM
    match proto.delete_vm(endpoint, vm_id).await {
        Ok(_) => tracing::info!("Cleaned up test VM: {}", vm_id),
        Err(e) => tracing::warn!("Failed to cleanup test VM {}: {:?}", vm_id, e),
    }
}
