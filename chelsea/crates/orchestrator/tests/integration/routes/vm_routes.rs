use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use dto_lib::chelsea_server2::vm::{
    VmBranchResponse, VmCommitResponse, VmCreateRequest, VmCreateVmConfig, VmFromCommitResponse,
    VmListAllResponse, VmNewRootResponse,
};
use dto_lib::orchestrator::vm::VmMetadataResponse;
use orchestrator::{db::ClustersRepository, inbound::routes::controlplane::vm::NewRootVmRequest};
use tower::util::ServiceExt; // for `oneshot`
use uuid::Uuid;

use super::common::setup_route_test;
use crate::skip_if_no_endpoint;

/// Comprehensive integration test for VM REST API routes
///
/// NOTE: This is structured as a single large test rather than multiple smaller tests
/// because the action::setup() function sets a global singleton context that persists
/// across test boundaries. When multiple test functions run sequentially, the second
/// test would inherit a stale action context with invalidated database prepared statements
/// from the first test's rollback, causing "prepared statement does not exist" errors.
///
/// By running all test scenarios within a single test function that holds one guard,
/// we ensure proper isolation and avoid the global state conflicts.
///
/// Test scenarios covered:
/// 1. POST /api/vm/new_root - Create new root VM
/// 2. POST /api/vm/{vm_id}/branch - Branch VM from parent
/// 3. POST /api/vm/{vm_id}/commit - Commit VM to snapshot
/// 4. POST /api/vm/from_commit - Restore VM from commit
/// 5. PATCH /api/vm/{vm_id}/state - Update VM state (pause/resume)
/// 6. GET /api/node/{node_id}/vms - List all VMs on node
/// 6b. GET /api/vm/{vm_id}/metadata - Get VM metadata
/// 7. DELETE /api/vm/{vm_id} - Delete VM
/// 8. Error cases (invalid UUIDs, not found, etc.)
///
/// See also: `test_authentication()` for authentication/authorization tests
///
/// Should be run with this command:
/// sudo DATABASE_URL="postgresql://postgres:opensesame@127.0.0.1:5432/vers" \
/// CHELSEA_TEST_ENDPOINT=127.0.0.1 \
/// CHELSEA_SERVER_PORT=8111 \
/// HOME=/home/ubuntu \
/// /home/ubuntu/.cargo/bin/cargo test \
/// --package orchestrator \
/// --features integration-tests \
/// --test mod \
/// test_vm_routes_comprehensive \
/// -- --nocapture

#[tokio::test]
async fn test_vm_routes_comprehensive() {
    skip_if_no_endpoint!();

    // We'll reuse the router by cloning it for each request
    // Note: Axum's Router can't be cloned directly, so we'll need to rebuild it for each test
    // or use the oneshot pattern. For simplicity, we'll use a helper to make requests.

    let auth_header = "Bearer kAiByMOc1nLKdIqoHD7PrNopJdG3LO3f";

    // ========================================================================
    // Scenario 1: POST /api/vm/new_root - Create new root VM
    // ========================================================================
    tracing::info!("Scenario 1: POST /api/vm/new_root - Create new root VM");

    let (router, db, cluster_id, node_id, test_endpoint) = setup_route_test().await;

    tracing::info!(
        cluster_id = %cluster_id,
        node_id = %node_id,
        endpoint = %test_endpoint,
        "Starting comprehensive VM routes integration test"
    );

    let body = NewRootVmRequest {
        vm_config: VmCreateRequest {
            vm_config: VmCreateVmConfig {
                kernel_name: Some("default.bin".to_string()),
                image_name: Some("default".to_string()),
                vcpu_count: Some(2),
                mem_size_mib: Some(512),
                fs_size_mib: Some(1024),
            },
        },
        cluster_id: cluster_id.to_string(),
    };

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("Content-Type", "application/json")
        .header("Authorization", auth_header)
        .body(serde_json::to_string(&body).unwrap())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "Should create VM successfully"
    );

    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_response: VmNewRootResponse =
        serde_json::from_slice(&bytes).expect("Response should be VmNewRootResponse");

    let vm_id = Uuid::parse_str(&create_response.id).expect("Should be valid UUID");
    tracing::info!("✓ Created VM via POST /api/vm/new_root: {}", vm_id);

    // ========================================================================
    // Scenario 2: POST /api/vm/{vm_id}/branch - Branch VM from parent
    // ========================================================================
    tracing::info!("Scenario 2: POST /api/vm/{{vm_id}}/branch - Branch VM");

    let (router, db, cluster_id, node_id, _) = setup_route_test().await;

    // First create a parent VM
    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "vm_config": {
            "kernel_name": "default.bin",
            "image_name": "default",
            "vcpu_count": 2,
            "mem_size_mib": 512,
            "fs_size_mib": 1024
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let parent_response: VmNewRootResponse = serde_json::from_slice(&bytes).unwrap();
    let parent_vm_id = Uuid::parse_str(&parent_response.id).unwrap();

    // Now branch from the parent
    let (router, db, _, _, _) = setup_route_test().await;

    let req = Request::builder()
        .method("POST")
        .uri(&format!("/api/v1/vm/{}/branch", parent_vm_id))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "Should branch VM successfully"
    );

    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let branch_response: VmBranchResponse =
        serde_json::from_slice(&bytes).expect("Response should be VmBranchResponse");

    let child_vm_id = Uuid::parse_str(&branch_response.vm_id).expect("Should be valid UUID");
    tracing::info!(
        "✓ Branched VM via POST /api/vm/{{vm_id}}/branch: {} -> {}",
        parent_vm_id,
        child_vm_id
    );

    // ========================================================================
    // Scenario 3: POST /api/vm/{vm_id}/commit - Commit VM to snapshot
    // ========================================================================
    tracing::info!("Scenario 3: POST /api/vm/{{vm_id}}/commit - Commit VM");

    let (router, db, cluster_id, _, _) = setup_route_test().await;

    // Create a VM to commit
    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "vm_config": {
            "kernel_name": "default.bin",
            "image_name": "default",
            "vcpu_count": 2,
            "mem_size_mib": 512,
            "fs_size_mib": 1024
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_response: VmNewRootResponse = serde_json::from_slice(&bytes).unwrap();
    let vm_id = Uuid::parse_str(&create_response.id).unwrap();

    // Now commit it
    let (router, _, _, _, _) = setup_route_test().await;

    let req = Request::builder()
        .method("POST")
        .uri(&format!("/api/v1/vm/{}/commit", vm_id))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "Should commit VM successfully"
    );

    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let commit_response: VmCommitResponse =
        serde_json::from_slice(&bytes).expect("Response should be VmCommitResponse");

    let commit_id = Uuid::parse_str(&commit_response.commit_id).expect("Should be valid UUID");
    tracing::info!(
        "✓ Committed VM via POST /api/vm/{{vm_id}}/commit: {} -> {}",
        vm_id,
        commit_id
    );
    tracing::info!("  Architecture: {}", commit_response.host_architecture);

    // ========================================================================
    // Scenario 4: POST /api/vm/from_commit - Restore VM from commit
    // ========================================================================
    tracing::info!("Scenario 4: POST /api/vm/from_commit - Restore VM from commit");

    let (router, db, cluster_id, _, _) = setup_route_test().await;

    // First create and commit a VM
    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "vm_config": {
            "kernel_name": "default.bin",
            "image_name": "default",
            "vcpu_count": 2,
            "mem_size_mib": 512,
            "fs_size_mib": 1024
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_response: VmNewRootResponse = serde_json::from_slice(&bytes).unwrap();
    let vm_id = Uuid::parse_str(&create_response.id).unwrap();

    // Commit it
    let (router, _, cluster_id, _, _) = setup_route_test().await;

    let req = Request::builder()
        .method("POST")
        .uri(&format!("/api/v1/vm/{}/commit", vm_id))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let commit_response: VmCommitResponse = serde_json::from_slice(&bytes).unwrap();
    let commit_id = Uuid::parse_str(&commit_response.commit_id).unwrap();

    // Now restore from commit
    let (router, _, _, _, _) = setup_route_test().await;

    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "commit_id": commit_id.to_string()
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/from_commit")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "Should restore VM from commit successfully"
    );

    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let restore_response: VmFromCommitResponse =
        serde_json::from_slice(&bytes).expect("Response should be VmFromCommitResponse");

    let restored_vm_id = Uuid::parse_str(&restore_response.vm_id).expect("Should be valid UUID");
    tracing::info!(
        "✓ Restored VM via POST /api/vm/from_commit: commit {} -> VM {}",
        commit_id,
        restored_vm_id
    );

    // ========================================================================
    // Scenario 5: PATCH /api/vm/{vm_id}/state - Update VM state
    // ========================================================================
    tracing::info!("Scenario 5: PATCH /api/vm/{{vm_id}}/state - Update VM state");

    let (router, db, cluster_id, _, _) = setup_route_test().await;

    // Create a VM to test state changes
    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "vm_config": {
            "kernel_name": "default.bin",
            "image_name": "default",
            "vcpu_count": 2,
            "mem_size_mib": 512,
            "fs_size_mib": 1024
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_response: VmNewRootResponse = serde_json::from_slice(&bytes).unwrap();
    let vm_id = Uuid::parse_str(&create_response.id).unwrap();

    // Pause the VM
    let (router, _, _, _, _) = setup_route_test().await;

    let body = serde_json::json!({
        "state": "Paused"
    })
    .to_string();

    let req = Request::builder()
        .method("PATCH")
        .uri(&format!("/api/v1/vm/{}/state", vm_id))
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Should pause VM successfully"
    );
    tracing::info!("✓ Paused VM via PATCH /api/vm/{{vm_id}}/state: {}", vm_id);

    // Resume the VM
    let (router, _, _, _, _) = setup_route_test().await;

    let body = serde_json::json!({
        "state": "Running"
    })
    .to_string();

    let req = Request::builder()
        .method("PATCH")
        .uri(&format!("/api/v1/vm/{}/state", vm_id))
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Should resume VM successfully"
    );
    tracing::info!("✓ Resumed VM via PATCH /api/vm/{{vm_id}}/state: {}", vm_id);

    // ========================================================================
    // Scenario 6: GET /api/node/{node_id}/vms - List all VMs on node
    // ========================================================================
    tracing::info!("Scenario 6: GET /api/node/{{node_id}}/vms - List all VMs");

    let (router, db, cluster_id, node_id, _) = setup_route_test().await;

    // Create a VM so we have something to list
    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "vm_config": {
            "kernel_name": "default.bin",
            "image_name": "default",
            "vcpu_count": 2,
            "mem_size_mib": 512,
            "fs_size_mib": 1024
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_response: VmNewRootResponse = serde_json::from_slice(&bytes).unwrap();
    let vm_id = Uuid::parse_str(&create_response.id).unwrap();

    // Now list VMs on the node
    let (router, _, _, _, _) = setup_route_test().await;

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/api/v1/node/{}/vms", node_id))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Should list VMs successfully"
    );

    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let list_response: VmListAllResponse =
        serde_json::from_slice(&bytes).expect("Response should be VmListAllResponse");

    tracing::info!(
        "✓ Listed {} VMs via GET /api/node/{{node_id}}/vms",
        list_response.vms.len()
    );

    // Verify our VM is in the list
    let vm_id_str = vm_id.to_string();
    let found = list_response.vms.iter().any(|v| v.vm_id == vm_id_str);
    assert!(found, "Created VM should be in the list");
    tracing::info!("✓ Found created VM in list");

    // ========================================================================
    // Scenario 6b: GET /api/vm/{vm_id}/metadata - Get VM metadata
    // ========================================================================
    tracing::info!("Scenario 6b: GET /api/vm/{{vm_id}}/metadata - Get VM metadata");

    let (router, _, cluster_id, _, _) = setup_route_test().await;

    // Create a VM to get metadata for
    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "vm_config": {
            "kernel_name": "default.bin",
            "image_name": "default",
            "vcpu_count": 2,
            "mem_size_mib": 512,
            "fs_size_mib": 1024
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_response: VmNewRootResponse = serde_json::from_slice(&bytes).unwrap();
    let vm_id = Uuid::parse_str(&create_response.id).unwrap();

    // Now get the metadata
    let (router, _, _, _, _) = setup_route_test().await;

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/api/v1/vm/{}/metadata", vm_id))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Should get VM metadata successfully"
    );

    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let metadata_response: VmMetadataResponse =
        serde_json::from_slice(&bytes).expect("Response should be VmMetadataResponse");

    // Verify the metadata matches what we expect
    assert_eq!(metadata_response.vm_id, vm_id, "VM ID should match");
    assert!(metadata_response.deleted_at.is_none(), "VM should not be deleted");
    assert!(!metadata_response.ip.is_empty(), "VM should have an IP address");
    tracing::info!(
        "✓ Got VM metadata via GET /api/vm/{{vm_id}}/metadata: vm_id={}, ip={}, state={:?}",
        metadata_response.vm_id,
        metadata_response.ip,
        metadata_response.state
    );

    // ========================================================================
    // Scenario 7: DELETE /api/vm/{vm_id} - Delete VM
    // ========================================================================
    tracing::info!("Scenario 7: DELETE /api/vm/{{vm_id}} - Delete VM");

    let (router, db, cluster_id, _, _) = setup_route_test().await;

    // Create a VM to delete
    let body = serde_json::json!({
        "cluster_id": cluster_id.to_string(),
        "vm_config": {
            "kernel_name": "default.bin",
            "image_name": "default",
            "vcpu_count": 2,
            "mem_size_mib": 512,
            "fs_size_mib": 1024
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let create_response: VmNewRootResponse = serde_json::from_slice(&bytes).unwrap();
    let vm_id = Uuid::parse_str(&create_response.id).unwrap();

    // Now delete it
    let (router, _, _, _, _) = setup_route_test().await;

    let req = Request::builder()
        .method("DELETE")
        .uri(&format!("/api/v1/vm/{}", vm_id))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Should delete VM successfully"
    );

    let bytes = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let delete_response: serde_json::Value =
        serde_json::from_slice(&bytes).expect("Response should be JSON");

    let deleted_ids = delete_response["deleted_ids"]
        .as_array()
        .expect("Should have deleted_ids array");
    assert_eq!(deleted_ids.len(), 1, "Should delete exactly 1 VM");
    tracing::info!("✓ Deleted VM via DELETE /api/vm/{{vm_id}}: {}", vm_id);

    // ========================================================================
    // Scenario 8: Error cases
    // ========================================================================
    tracing::info!("Scenario 8: Error cases");

    let (router, _, _, _, _) = setup_route_test().await;

    // Test invalid cluster_id format
    let body = serde_json::json!({
        "cluster_id": "not-a-uuid",
        "vm_config": {
            "kernel_name": "default.bin"
        }
    })
    .to_string();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/vm/new_root")
        .header("content-type", "application/json")
        .header("Authorization", auth_header)
        .body(Body::from(body))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "Should reject invalid cluster_id"
    );
    tracing::info!("✓ Correctly rejected invalid cluster_id with 400");

    // Test VM not found
    let (router, _, _, _, _) = setup_route_test().await;
    let nonexistent_vm = Uuid::new_v4();

    let req = Request::builder()
        .method("DELETE")
        .uri(&format!("/api/v1/vm/{}", nonexistent_vm))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "Should return 404 for nonexistent VM"
    );
    tracing::info!("✓ Correctly returned 404 for nonexistent VM");

    // Test node not found for list VMs
    let (router, _, _, _, _) = setup_route_test().await;
    let nonexistent_node = Uuid::new_v4();

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/api/v1/node/{}/vms", nonexistent_node))
        .header("Authorization", auth_header)
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "Should return 404 for nonexistent node"
    );
    tracing::info!("✓ Correctly returned 404 for nonexistent node");

    // ========================================================================
    // Test Complete
    // ========================================================================
    tracing::info!("✓ All scenarios completed successfully (all VM REST API routes tested)");
    db.rollback_for_test().await.unwrap();
}

/// Test authentication and authorization
/// This is a separate test because it doesn't mutate state and doesn't need
/// the full VM lifecycle testing. It just tests that auth middleware works.
///
/// NOTE: Like the comprehensive test, we do all tests in one function to avoid
/// prepared statement conflicts from multiple setup_route_test() calls.
#[tokio::test]
async fn test_authentication() {
    skip_if_no_endpoint!();

    tracing::info!("Testing authentication on VM routes");

    // Setup once and reuse for all auth tests
    let (router, db, _cluster_id, node_id, _test_endpoint) = setup_route_test().await;

    // ========================================================================
    // Test 1: Missing Authorization header
    // ========================================================================
    tracing::info!("Test 1: Missing Authorization header");

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/api/v1/node/{}/vms", node_id))
        .body(Body::empty())
        .unwrap();

    let resp = ServiceExt::<Request<Body>>::oneshot(&mut router.clone(), req)
        .await
        .unwrap();
    let status = resp.status();
    tracing::info!("Response status for missing auth: {}", status);
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "Should return 401 when Authorization header is missing"
    );
    tracing::info!("✓ Correctly rejected request with missing Authorization header (401)");

    // ========================================================================
    // Test 2: Invalid Authorization header format (no Bearer prefix)
    // ========================================================================
    tracing::info!("Test 2: Invalid Authorization header format");

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/api/v1/node/{}/vms", node_id))
        .header("Authorization", "InvalidTokenFormat")
        .body(Body::empty())
        .unwrap();

    let resp = ServiceExt::<Request<Body>>::oneshot(&mut router.clone(), req)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Should return 401 for invalid token format"
    );
    tracing::info!("✓ Correctly rejected invalid token format (401)");

    // ========================================================================
    // Test 3: Invalid Bearer token
    // ========================================================================
    tracing::info!("Test 3: Invalid Bearer token");

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/api/v1/node/{}/vms", node_id))
        .header("Authorization", "Bearer invalid_token_that_does_not_exist")
        .body(Body::empty())
        .unwrap();

    let resp = ServiceExt::<Request<Body>>::oneshot(&mut router.clone(), req)
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Should return 401 for invalid bearer token"
    );
    tracing::info!("✓ Correctly rejected invalid bearer token (401)");

    // ========================================================================
    // Test 4: Valid token should work (sanity check)
    // ========================================================================
    tracing::info!("Test 4: Valid token should work");

    let req = Request::builder()
        .method("GET")
        .uri(&format!("/api/v1/node/{}/vms", node_id))
        .header("Authorization", "Bearer kAiByMOc1nLKdIqoHD7PrNopJdG3LO3f")
        .body(Body::empty())
        .unwrap();

    let resp = ServiceExt::<Request<Body>>::oneshot(&mut router.clone(), req)
        .await
        .unwrap();
    // Should be 200 OK or 404 NOT_FOUND (node might not exist), but NOT 401
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Valid token should not return 401"
    );
    tracing::info!("✓ Valid token accepted (status: {})", resp.status());

    // ========================================================================
    // Test Complete
    // ========================================================================
    tracing::info!("✓ All authentication tests passed");
    db.rollback_for_test().await.unwrap();
}
