//! Tests for the POST /internal/vm/{vm_id}/boot-failed endpoint
//!
//! These tests verify the Chelsea boot failure callback that marks VMs as
//! deleted in the orchestrator DB.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::Utc;
use orch_test::ActionTestEnv;
use orchestrator::db::VMsRepository;
use tower::ServiceExt;
use uuid::Uuid;
use vers_config::VersConfig;

/// Test the POST /internal/vm/{vm_id}/boot-failed endpoint
/// marks a VM as deleted when called with a valid admin key.
#[test]
fn boot_failed_endpoint_marks_vm_deleted() {
    ActionTestEnv::with_env(|env| async move {
        let vm_id = Uuid::new_v4();
        let node_id: Uuid = "4569f1fe-054b-4e8d-855a-f3545167f8a9".parse().unwrap();
        let owner_id: Uuid = "ef90fd52-66b5-47e7-b7dc-e73c4381028f".parse().unwrap();

        env.db()
            .vms()
            .insert(
                vm_id,
                None,
                None,
                node_id,
                "fd00:fe11:deed:1::99".parse().unwrap(),
                "fake_private_key".to_string(),
                "fake_public_key".to_string(),
                51899,
                owner_id,
                Utc::now(),
                None,
                4,
                512,
            )
            .await
            .expect("Failed to insert VM");

        // Verify VM exists
        let vm_before = env.db().vms().get_by_id(vm_id).await.expect("DB error");
        assert!(vm_before.is_some(), "VM should exist before callback");

        // Call the internal boot-failed endpoint with admin key
        let admin_key = &VersConfig::orchestrator().admin_api_key;
        let uri = format!("/api/v1/internal/vm/{}/boot-failed", vm_id);
        let req = Request::builder()
            .method(Method::POST)
            .uri(&uri)
            .header("Authorization", format!("Bearer {}", admin_key))
            .body(Body::empty())
            .unwrap();

        let routes = env.inbound();
        let response = routes.oneshot(req).await.expect("request failed");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "boot-failed endpoint should return 200 OK"
        );

        // VM should now be soft-deleted
        let vm_after = env.db().vms().get_by_id(vm_id).await.expect("DB error");
        assert!(
            vm_after.is_none(),
            "VM should be soft-deleted after boot-failed callback"
        );
    });
}

/// Test that the boot-failed endpoint rejects requests without admin key.
#[test]
fn boot_failed_endpoint_rejects_without_admin_key() {
    ActionTestEnv::with_env(|env| async move {
        let vm_id = Uuid::new_v4();
        let uri = format!("/api/v1/internal/vm/{}/boot-failed", vm_id);

        // No Authorization header
        let req = Request::builder()
            .method(Method::POST)
            .uri(&uri)
            .body(Body::empty())
            .unwrap();

        let routes = env.inbound();
        let response = routes.oneshot(req).await.expect("request failed");

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "boot-failed endpoint should reject requests without admin key"
        );
    });
}

/// Test that the boot-failed endpoint rejects requests with wrong admin key.
#[test]
fn boot_failed_endpoint_rejects_wrong_admin_key() {
    ActionTestEnv::with_env(|env| async move {
        let vm_id = Uuid::new_v4();
        let uri = format!("/api/v1/internal/vm/{}/boot-failed", vm_id);

        let req = Request::builder()
            .method(Method::POST)
            .uri(&uri)
            .header("Authorization", "Bearer wrong-key-here")
            .body(Body::empty())
            .unwrap();

        let routes = env.inbound();
        let response = routes.oneshot(req).await.expect("request failed");

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "boot-failed endpoint should reject wrong admin key"
        );
    });
}
