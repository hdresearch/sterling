use std::net::IpAddr;
use std::str::FromStr;

use orchestrator::outbound::node_proto::ChelseaProto;

use crate::skip_if_no_endpoint;
use super::common::get_test_endpoint;

#[tokio::test]
async fn test_system_telemetry_success() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let result = proto.system_telemetry(endpoint).await;

    match result {
        Ok(telemetry) => {
            tracing::info!("System telemetry retrieved successfully");
            tracing::info!("RAM: total={} MiB, available={} MiB",
                telemetry.ram.real_mib_total,
                telemetry.ram.real_mib_available
            );
            tracing::info!("CPU: total={:.2}%, available={:.2}%",
                telemetry.cpu.real_total,
                telemetry.cpu.real_available
            );
            tracing::info!("FS: total={} MiB, available={} MiB",
                telemetry.fs.mib_total,
                telemetry.fs.mib_available
            );
            tracing::info!("VMs: current={}, max={}",
                telemetry.chelsea.vm_count_current,
                telemetry.chelsea.vm_count_max
            );

            // Basic sanity checks
            assert!(
                telemetry.ram.real_mib_total > 0,
                "RAM total should be greater than 0"
            );
            assert!(
                telemetry.ram.real_mib_available <= telemetry.ram.real_mib_total,
                "RAM available should be less than or equal to total"
            );
            assert!(
                telemetry.cpu.real_total > 0.0,
                "CPU total should be greater than 0"
            );
            assert!(
                telemetry.fs.mib_available <= telemetry.fs.mib_total,
                "FS available should be less than or equal to total"
            );
            assert!(
                telemetry.chelsea.vm_count_current <= telemetry.chelsea.vm_count_max,
                "Current VM count should not exceed max"
            );
        }
        Err(e) => {
            panic!("Expected successful telemetry fetch, but got error: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_system_telemetry_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let proto = ChelseaProto::new();
    let result = proto.system_telemetry(endpoint).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_system_telemetry_response_structure() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let result = proto.system_telemetry(endpoint).await;

    match result {
        Ok(telemetry) => {
            // Validate that all expected fields are present and reasonable
            tracing::info!("Validating telemetry structure: {:?}", telemetry);

            // RAM validation
            assert!(
                telemetry.ram.vm_mib_total <= telemetry.ram.real_mib_total,
                "VM RAM total should not exceed real RAM total"
            );

            // CPU validation
            assert!(
                telemetry.cpu.vcpu_count_total > 0,
                "vCPU count should be greater than 0"
            );
            assert!(
                telemetry.cpu.vcpu_count_vm_available <= telemetry.cpu.vcpu_count_total,
                "Available vCPU count should not exceed total"
            );

            // FS validation
            assert!(
                telemetry.fs.mib_total > 0,
                "FS total should be greater than 0"
            );

            // Chelsea validation
            assert!(
                telemetry.chelsea.vm_count_max > 0,
                "Max VM count should be greater than 0"
            );

            tracing::info!("All telemetry structure validations passed");
        }
        Err(e) => {
            panic!("Expected successful telemetry response to validate: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_system_telemetry_multiple_calls() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Make multiple telemetry calls in sequence
    let mut previous_vm_count = None;
    for i in 0..3 {
        let result = proto.system_telemetry(endpoint).await;
        assert!(
            result.is_ok(),
            "Telemetry call {} should succeed",
            i
        );

        if let Ok(telemetry) = result {
            tracing::info!(
                "Telemetry call {}: {} VMs out of {} max",
                i,
                telemetry.chelsea.vm_count_current,
                telemetry.chelsea.vm_count_max
            );

            // VM count should be stable or change (but not go negative)
            if let Some(prev_count) = previous_vm_count {
                let current_count = telemetry.chelsea.vm_count_current;
                tracing::info!(
                    "VM count change: {} -> {}",
                    prev_count,
                    current_count
                );
            }
            previous_vm_count = Some(telemetry.chelsea.vm_count_current);
        }

        // Small delay between checks
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn test_system_telemetry_concurrent_calls() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Make multiple concurrent telemetry calls
    let mut handles = vec![];
    for i in 0..3 {
        let proto_clone = ChelseaProto::new();
        let handle = tokio::spawn(async move {
            proto_clone.system_telemetry(endpoint).await
        });
        handles.push((i, handle));
    }

    // Wait for all to complete
    for (i, handle) in handles {
        let result = handle.await.expect("Task should not panic");
        assert!(
            result.is_ok(),
            "Concurrent telemetry call {} should succeed",
            i
        );

        if let Ok(telemetry) = result {
            tracing::info!(
                "Concurrent call {}: RAM={} MiB, VMs={}/{}",
                i,
                telemetry.ram.real_mib_available,
                telemetry.chelsea.vm_count_current,
                telemetry.chelsea.vm_count_max
            );
        }
    }
}
