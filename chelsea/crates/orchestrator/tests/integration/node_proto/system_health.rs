use std::net::IpAddr;
use std::str::FromStr;

use orchestrator::outbound::node_proto::ChelseaProto;

use crate::skip_if_no_endpoint;
use super::common::get_test_endpoint;

#[tokio::test]
async fn test_system_health_success() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();
    let result = proto.system_health(endpoint).await;

    match result {
        Ok(()) => {
            tracing::info!("System health check passed");
        }
        Err(e) => {
            panic!("Expected successful health check, but got error: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_system_health_invalid_endpoint() {
    // Test with an endpoint that doesn't exist
    let endpoint = IpAddr::from_str("192.0.2.1").unwrap(); // TEST-NET-1

    let proto = ChelseaProto::new();
    let result = proto.system_health(endpoint).await;

    // Should fail with either timeout or connection refused
    assert!(
        result.is_err(),
        "Expected error when connecting to invalid endpoint"
    );
    tracing::info!("Correctly failed with invalid endpoint: {:?}", result);
}

#[tokio::test]
async fn test_system_health_multiple_calls() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    let proto = ChelseaProto::new();

    // Make multiple health check calls in sequence
    for i in 0..5 {
        let result = proto.system_health(endpoint).await;
        assert!(
            result.is_ok(),
            "Health check {} should succeed",
            i
        );
        tracing::info!("Health check {} passed", i);

        // Small delay between checks
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn test_system_health_concurrent_calls() {
    skip_if_no_endpoint!();
    let endpoint = get_test_endpoint().unwrap();

    // Make multiple concurrent health check calls
    let mut handles = vec![];
    for i in 0..5 {
        let proto_clone = ChelseaProto::new();
        let handle = tokio::spawn(async move {
            proto_clone.system_health(endpoint).await
        });
        handles.push((i, handle));
    }

    // Wait for all to complete
    for (i, handle) in handles {
        let result = handle.await.expect("Task should not panic");
        assert!(
            result.is_ok(),
            "Concurrent health check {} should succeed",
            i
        );
        tracing::info!("Concurrent health check {} passed", i);
    }
}
