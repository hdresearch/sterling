//! End-to-end integration tests for base images API.
//!
//! These tests make actual HTTP requests to the orchestrator,
//! which then communicates with Chelsea to create base images.
//!
//! Prerequisites:
//! - Single-node environment running (./scripts/single-node.sh start)
//! - All services healthy: PostgreSQL, Ceph, Chelsea, Orchestrator
//!
//! Run with:
//! ```bash
//! cargo test --package orchestrator --test base_images_e2e -- --nocapture --test-threads=1
//! ```

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Orchestrator endpoint for the single-node environment (via WireGuard)
const ORCHESTRATOR_URL: &str = "http://[fd00:fe11:deed::ffff]:8090";

/// Test API key from the seed data (see pg/migrations/20251111063619_seed_db.sql)
const TEST_API_KEY: &str = "ef90fd52-66b5-47e7-b7dc-e73c4381028fbfa85827e1f1ebab3078c3d3249a72647aef57451bd5feac7b727dcb5842590c";

/// Request body for creating a base image from docker
#[derive(Debug, Serialize)]
struct CreateBaseImageRequest {
    image_name: String,
    source: ImageSource,
    size_mib: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ImageSource {
    Docker { image_ref: String },
}

/// Response from creating a base image
#[derive(Debug, Deserialize)]
struct CreateBaseImageResponse {
    job_id: String,
    #[allow(dead_code)] // Used by Deserialize.
    image_name: String,
    status: String,
}

/// Response from listing base images
#[derive(Debug, Deserialize)]
struct ListBaseImagesResponse {
    images: Vec<BaseImageInfo>,
    total: i64,
    #[allow(dead_code)] // Used by Deserialize.
    limit: i64,
    #[allow(dead_code)] // Used by Deserialize.
    offset: i64,
}

#[derive(Debug, Deserialize)]
struct BaseImageInfo {
    #[allow(dead_code)] // Used by Deserialize.
    base_image_id: String,
    image_name: String,
    #[allow(dead_code)] // Used by Deserialize.
    owner_id: String,
    #[allow(dead_code)] // Used by Deserialize.
    is_public: bool,
    source_type: String,
    size_mib: i32,
    #[allow(dead_code)] // Used by Deserialize.
    description: Option<String>,
    #[allow(dead_code)] // Used by Deserialize.
    created_at: String,
}

/// Response from getting image status
#[derive(Debug, Deserialize)]
struct BaseImageStatusResponse {
    #[allow(dead_code)] // Used by Deserialize.
    image_name: String,
    status: String,
    size_mib: i32,
    error_message: Option<String>,
}

fn create_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("Failed to create HTTP client")
}

/// Check if the orchestrator is available
async fn check_orchestrator_available() -> bool {
    let client = create_client();
    // Try to hit any endpoint - even a 401 means the orchestrator is up
    match client
        .get(format!("{}/api/v1/images", ORCHESTRATOR_URL))
        .send()
        .await
    {
        Ok(resp) => {
            // 401 Unauthorized is expected without auth, but means orchestrator is up
            resp.status().as_u16() == 401 || resp.status().is_success()
        }
        Err(_) => false,
    }
}

macro_rules! skip_if_no_orchestrator {
    () => {
        if !check_orchestrator_available().await {
            eprintln!(
                "\n⚠️  Skipping test: Orchestrator not available at {}\n\
                 To run this test, start the single-node environment:\n\
                 ./scripts/single-node.sh start\n",
                ORCHESTRATOR_URL
            );
            return;
        }
    };
}

#[tokio::test]
async fn test_list_images_e2e() {
    skip_if_no_orchestrator!();

    let client = create_client();

    let response = client
        .get(format!("{}/api/v1/images", ORCHESTRATOR_URL))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .send()
        .await
        .expect("Failed to send request");

    println!("Status: {}", response.status());

    if response.status().is_success() {
        let body: ListBaseImagesResponse = response.json().await.expect("Failed to parse response");
        println!(
            "✓ Listed {} images (total: {})",
            body.images.len(),
            body.total
        );
        for img in &body.images {
            println!(
                "  - {} (source: {}, size: {} MiB)",
                img.image_name, img.source_type, img.size_mib
            );
        }
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        panic!("Failed to list images: {} - {}", status, text);
    }
}

#[tokio::test]
async fn test_create_docker_image_e2e() {
    skip_if_no_orchestrator!();

    let client = create_client();
    let image_name = format!("test-alpine-{}", chrono::Utc::now().timestamp());

    let request = CreateBaseImageRequest {
        image_name: image_name.clone(),
        source: ImageSource::Docker {
            image_ref: "alpine:latest".to_string(),
        },
        size_mib: 512,
        description: Some("E2E test image from alpine".to_string()),
    };

    println!(
        "Creating base image '{}' from docker alpine:latest...",
        image_name
    );

    let response = client
        .post(format!("{}/api/v1/images/create", ORCHESTRATOR_URL))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    let status = response.status();
    println!("Status: {}", status);

    if status.is_success() || status.as_u16() == 201 {
        let body: CreateBaseImageResponse =
            response.json().await.expect("Failed to parse response");
        println!(
            "✓ Image creation job started: job_id={}, status={}",
            body.job_id, body.status
        );

        // Poll for completion (with timeout)
        println!("Polling for image creation completion...");
        let mut attempts = 0;
        let max_attempts = 60; // 5 minutes with 5-second intervals

        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            attempts += 1;

            let status_response = client
                .get(format!(
                    "{}/api/v1/images/{}/status",
                    ORCHESTRATOR_URL, image_name
                ))
                .header("Authorization", format!("Bearer {}", TEST_API_KEY))
                .send()
                .await
                .expect("Failed to send status request");

            if status_response.status().is_success() {
                let status_body: BaseImageStatusResponse = status_response
                    .json()
                    .await
                    .expect("Failed to parse status");
                println!(
                    "  [{}/{}] Status: {} (size: {} MiB)",
                    attempts, max_attempts, status_body.status, status_body.size_mib
                );

                if status_body.status == "completed" {
                    println!("✓ Image creation completed successfully!");
                    break;
                } else if status_body.status == "failed" {
                    panic!(
                        "Image creation failed: {}",
                        status_body.error_message.unwrap_or_default()
                    );
                }
            } else {
                let err_status = status_response.status();
                let err_text = status_response.text().await.unwrap_or_default();
                println!(
                    "  [{}/{}] Status check failed: {} - {}",
                    attempts, max_attempts, err_status, err_text
                );
            }

            if attempts >= max_attempts {
                panic!("Timeout waiting for image creation to complete");
            }
        }

        // Verify the image appears in the list
        let list_response = client
            .get(format!("{}/api/v1/images", ORCHESTRATOR_URL))
            .header("Authorization", format!("Bearer {}", TEST_API_KEY))
            .send()
            .await
            .expect("Failed to send list request");

        if list_response.status().is_success() {
            let list_body: ListBaseImagesResponse = list_response
                .json()
                .await
                .expect("Failed to parse list response");
            let found = list_body
                .images
                .iter()
                .any(|img| img.image_name == image_name);
            if found {
                println!("✓ Image found in list");
            } else {
                println!("⚠️  Image not found in list (may be expected if test isolated)");
            }
        }
    } else {
        let text = response.text().await.unwrap_or_default();
        panic!("Failed to create image: {} - {}", status, text);
    }
}

#[tokio::test]
async fn test_get_nonexistent_image_status_e2e() {
    skip_if_no_orchestrator!();

    let client = create_client();

    let response = client
        .get(format!(
            "{}/api/v1/images/nonexistent-image-12345/status",
            ORCHESTRATOR_URL
        ))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .send()
        .await
        .expect("Failed to send request");

    let status = response.status();
    println!("Status: {}", status);

    assert_eq!(
        status.as_u16(),
        404,
        "Expected 404 for nonexistent image, got {}",
        status
    );
    println!("✓ Correctly returned 404 for nonexistent image");
}

#[tokio::test]
async fn test_unauthorized_request_e2e() {
    skip_if_no_orchestrator!();

    let client = create_client();

    // Test without auth header
    let response = client
        .get(format!("{}/api/v1/images", ORCHESTRATOR_URL))
        .send()
        .await
        .expect("Failed to send request");

    let status = response.status();
    println!("Status (no auth): {}", status);
    assert_eq!(
        status.as_u16(),
        401,
        "Expected 401 without auth, got {}",
        status
    );

    // Test with invalid auth
    let response = client
        .get(format!("{}/api/v1/images", ORCHESTRATOR_URL))
        .header("Authorization", "Bearer invalid-key-12345")
        .send()
        .await
        .expect("Failed to send request");

    let status = response.status();
    println!("Status (invalid auth): {}", status);
    assert_eq!(
        status.as_u16(),
        401,
        "Expected 401 with invalid auth, got {}",
        status
    );

    println!("✓ Correctly returned 401 for unauthorized requests");
}

#[tokio::test]
async fn test_duplicate_image_name_rejected_e2e() {
    skip_if_no_orchestrator!();

    let client = create_client();
    let image_name = format!("test-duplicate-{}", chrono::Utc::now().timestamp());

    let request = CreateBaseImageRequest {
        image_name: image_name.clone(),
        source: ImageSource::Docker {
            image_ref: "alpine:latest".to_string(),
        },
        size_mib: 512,
        description: None,
    };

    // First creation should succeed
    let response1 = client
        .post(format!("{}/api/v1/images/create", ORCHESTRATOR_URL))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .json(&request)
        .send()
        .await
        .expect("Failed to send first request");

    let status1 = response1.status();
    println!("First creation status: {}", status1);
    assert!(
        status1.is_success() || status1.as_u16() == 201,
        "First creation should succeed, got {}",
        status1
    );

    // Wait a moment for the job to be registered
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Second creation with same name should fail with 409 Conflict
    let response2 = client
        .post(format!("{}/api/v1/images/create", ORCHESTRATOR_URL))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .json(&request)
        .send()
        .await
        .expect("Failed to send second request");

    let status2 = response2.status();
    println!("Second creation status: {}", status2);
    assert_eq!(
        status2.as_u16(),
        409,
        "Expected 409 Conflict for duplicate name, got {}",
        status2
    );

    println!("✓ Correctly rejected duplicate image name with 409 Conflict");
}

/// Request body for creating a new VM
#[derive(Debug, Serialize)]
struct NewRootRequest {
    vm_config: VmConfig,
}

#[derive(Debug, Serialize)]
struct VmConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    image_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vcpu_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mem_size_mib: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fs_size_mib: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct NewVmResponse {
    vm_id: String,
}

#[derive(Debug, Deserialize)]
struct VmStatusResponse {
    #[allow(dead_code)] // Used by Deserialize.
    vm_id: String,
    state: String,
    #[serde(default)]
    #[allow(dead_code)] // Used by Deserialize.
    error_message: Option<String>,
}

/// Test that creates a base image from docker and boots a VM from it.
/// This is the critical test for verifying docker-based images work end-to-end.
#[tokio::test]
async fn test_docker_image_vm_boot_e2e() {
    skip_if_no_orchestrator!();

    let client = create_client();
    let image_name = format!("test-boot-{}", chrono::Utc::now().timestamp());

    // Step 1: Create a base image from alpine (smaller/faster than ubuntu)
    let create_request = CreateBaseImageRequest {
        image_name: image_name.clone(),
        source: ImageSource::Docker {
            image_ref: "alpine:latest".to_string(),
        },
        size_mib: 512,
        description: Some("E2E VM boot test image".to_string()),
    };

    println!(
        "Step 1: Creating base image '{}' from docker alpine:latest...",
        image_name
    );

    let response = client
        .post(format!("{}/api/v1/images/create", ORCHESTRATOR_URL))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .json(&create_request)
        .send()
        .await
        .expect("Failed to send create request");

    let status = response.status();
    assert!(
        status.is_success() || status.as_u16() == 201,
        "Image creation should succeed, got {}",
        status
    );

    let create_response: CreateBaseImageResponse =
        response.json().await.expect("Failed to parse response");
    println!(
        "✓ Image creation job started: job_id={}",
        create_response.job_id
    );

    // Step 2: Poll for image creation completion
    println!("Step 2: Waiting for image creation to complete...");
    let mut attempts = 0;
    let max_attempts = 60; // 5 minutes

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        attempts += 1;

        let status_response = client
            .get(format!(
                "{}/api/v1/images/{}/status",
                ORCHESTRATOR_URL, image_name
            ))
            .header("Authorization", format!("Bearer {}", TEST_API_KEY))
            .send()
            .await
            .expect("Failed to send status request");

        if status_response.status().is_success() {
            let status_body: BaseImageStatusResponse = status_response
                .json()
                .await
                .expect("Failed to parse status");
            println!(
                "  [{}/{}] Image status: {}",
                attempts, max_attempts, status_body.status
            );

            if status_body.status == "completed" {
                println!("✓ Image creation completed successfully");
                break;
            } else if status_body.status == "failed" {
                panic!(
                    "Image creation failed: {}",
                    status_body.error_message.unwrap_or_default()
                );
            }
        }

        if attempts >= max_attempts {
            panic!("Timeout waiting for image creation to complete");
        }
    }

    // Step 3: Create a VM from the custom image with wait_boot=true
    println!("Step 3: Creating VM from custom image with wait_boot=true...");

    let vm_request = NewRootRequest {
        vm_config: VmConfig {
            image_name: Some(image_name.clone()),
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            fs_size_mib: Some(1024),
        },
    };

    let vm_response = client
        .post(format!(
            "{}/api/v1/vm/new_root?wait_boot=true",
            ORCHESTRATOR_URL
        ))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .header("Host", "api.vers.sh")
        .json(&vm_request)
        .timeout(Duration::from_secs(120)) // VM boot can take a while
        .send()
        .await
        .expect("Failed to send VM create request");

    let vm_status = vm_response.status();
    println!("VM creation response status: {}", vm_status);

    if vm_status.is_success() || vm_status.as_u16() == 201 {
        let vm_info: NewVmResponse = vm_response
            .json()
            .await
            .expect("Failed to parse VM response");
        println!(
            "✓ VM created and booted successfully: vm_id={}",
            vm_info.vm_id
        );

        // Step 4: Verify VM status is "running"
        println!("Step 4: Verifying VM status...");
        let status_response = client
            .get(format!(
                "{}/api/v1/vm/{}/status",
                ORCHESTRATOR_URL, vm_info.vm_id
            ))
            .header("Authorization", format!("Bearer {}", TEST_API_KEY))
            .send()
            .await
            .expect("Failed to get VM status");

        if status_response.status().is_success() {
            let vm_status: VmStatusResponse = status_response
                .json()
                .await
                .expect("Failed to parse VM status");
            println!("VM state: {}", vm_status.state);

            // The VM should be running if wait_boot=true succeeded
            assert!(
                vm_status.state == "running" || vm_status.state == "paused",
                "Expected VM to be running or paused after boot, got: {}",
                vm_status.state
            );
            println!("✓ VM is in expected state: {}", vm_status.state);
        }

        // Step 5: Clean up - delete the VM
        println!("Step 5: Cleaning up VM...");
        let delete_response = client
            .delete(format!("{}/api/v1/vm/{}", ORCHESTRATOR_URL, vm_info.vm_id))
            .header("Authorization", format!("Bearer {}", TEST_API_KEY))
            .send()
            .await
            .expect("Failed to delete VM");

        if delete_response.status().is_success() {
            println!("✓ VM deleted successfully");
        } else {
            println!(
                "⚠️ Failed to delete VM (may need manual cleanup): {}",
                delete_response.status()
            );
        }

        println!("\n✓ Docker image VM boot test PASSED!");
    } else {
        let error_text = vm_response.text().await.unwrap_or_default();
        panic!(
            "VM creation failed with status {}: {}\n\
             This likely indicates a problem with the docker-based base image.\n\
             Check if the init system (systemd) is properly preserved during image creation.",
            vm_status, error_text
        );
    }
}

/// Test that creates a base image from ubuntu:24.04 docker image and boots a VM.
/// This tests the more complex case where the docker image has merged-usr layout
/// (symlinks for /bin, /sbin, /lib) that could conflict with the base squashfs.
#[tokio::test]
async fn test_ubuntu_docker_image_vm_boot_e2e() {
    skip_if_no_orchestrator!();

    let client = create_client();
    let image_name = format!("test-ubuntu-boot-{}", chrono::Utc::now().timestamp());

    // Step 1: Create a base image from ubuntu:24.04
    let create_request = CreateBaseImageRequest {
        image_name: image_name.clone(),
        source: ImageSource::Docker {
            image_ref: "ubuntu:24.04".to_string(),
        },
        size_mib: 1024, // Ubuntu is larger than alpine
        description: Some("E2E Ubuntu VM boot test image".to_string()),
    };

    println!(
        "Step 1: Creating base image '{}' from docker ubuntu:24.04...",
        image_name
    );

    let response = client
        .post(format!("{}/api/v1/images/create", ORCHESTRATOR_URL))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .json(&create_request)
        .send()
        .await
        .expect("Failed to send create request");

    let status = response.status();
    assert!(
        status.is_success() || status.as_u16() == 201,
        "Image creation should succeed, got {}",
        status
    );

    let create_response: CreateBaseImageResponse =
        response.json().await.expect("Failed to parse response");
    println!(
        "✓ Image creation job started: job_id={}",
        create_response.job_id
    );

    // Step 2: Poll for image creation completion (ubuntu takes longer)
    println!("Step 2: Waiting for image creation to complete...");
    let mut attempts = 0;
    let max_attempts = 120; // 10 minutes for ubuntu

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        attempts += 1;

        let status_response = client
            .get(format!(
                "{}/api/v1/images/{}/status",
                ORCHESTRATOR_URL, image_name
            ))
            .header("Authorization", format!("Bearer {}", TEST_API_KEY))
            .send()
            .await
            .expect("Failed to send status request");

        if status_response.status().is_success() {
            let status_body: BaseImageStatusResponse = status_response
                .json()
                .await
                .expect("Failed to parse status");
            println!(
                "  [{}/{}] Image status: {}",
                attempts, max_attempts, status_body.status
            );

            if status_body.status == "completed" {
                println!("✓ Image creation completed successfully");
                break;
            } else if status_body.status == "failed" {
                panic!(
                    "Image creation failed: {}",
                    status_body.error_message.unwrap_or_default()
                );
            }
        }

        if attempts >= max_attempts {
            panic!("Timeout waiting for image creation to complete");
        }
    }

    // Step 3: Create a VM from the ubuntu image with wait_boot=true
    println!("Step 3: Creating VM from ubuntu image with wait_boot=true...");
    println!("        (This will fail if the init system was not preserved)");

    let vm_request = NewRootRequest {
        vm_config: VmConfig {
            image_name: Some(image_name.clone()),
            vcpu_count: Some(1),
            mem_size_mib: Some(512),
            fs_size_mib: Some(2048), // Ubuntu needs more space
        },
    };

    let vm_response = client
        .post(format!(
            "{}/api/v1/vm/new_root?wait_boot=true",
            ORCHESTRATOR_URL
        ))
        .header("Authorization", format!("Bearer {}", TEST_API_KEY))
        .header("Host", "api.vers.sh")
        .json(&vm_request)
        .timeout(Duration::from_secs(180)) // Ubuntu boot takes longer
        .send()
        .await
        .expect("Failed to send VM create request");

    let vm_status = vm_response.status();
    println!("VM creation response status: {}", vm_status);

    if vm_status.is_success() || vm_status.as_u16() == 201 {
        let vm_info: NewVmResponse = vm_response
            .json()
            .await
            .expect("Failed to parse VM response");
        println!(
            "✓ VM created and booted successfully: vm_id={}",
            vm_info.vm_id
        );

        // Step 4: Verify VM status is "running"
        println!("Step 4: Verifying VM status...");
        let status_response = client
            .get(format!(
                "{}/api/v1/vm/{}/status",
                ORCHESTRATOR_URL, vm_info.vm_id
            ))
            .header("Authorization", format!("Bearer {}", TEST_API_KEY))
            .send()
            .await
            .expect("Failed to get VM status");

        if status_response.status().is_success() {
            let vm_status: VmStatusResponse = status_response
                .json()
                .await
                .expect("Failed to parse VM status");
            println!("VM state: {}", vm_status.state);

            assert!(
                vm_status.state == "running" || vm_status.state == "paused",
                "Expected VM to be running or paused after boot, got: {}",
                vm_status.state
            );
            println!("✓ VM is in expected state: {}", vm_status.state);
        }

        // Step 5: Clean up
        println!("Step 5: Cleaning up VM...");
        let delete_response = client
            .delete(format!("{}/api/v1/vm/{}", ORCHESTRATOR_URL, vm_info.vm_id))
            .header("Authorization", format!("Bearer {}", TEST_API_KEY))
            .send()
            .await
            .expect("Failed to delete VM");

        if delete_response.status().is_success() {
            println!("✓ VM deleted successfully");
        } else {
            println!(
                "⚠️ Failed to delete VM (may need manual cleanup): {}",
                delete_response.status()
            );
        }

        println!("\n✓ Ubuntu docker image VM boot test PASSED!");
    } else {
        let error_text = vm_response.text().await.unwrap_or_default();
        panic!(
            "Ubuntu VM boot FAILED with status {}: {}\n\n\
             This indicates that the ubuntu:24.04 docker image is breaking the init system.\n\
             The likely cause is that rsync is overwriting/removing critical boot files\n\
             (like /sbin/init -> systemd symlink) during the merge process.\n\n\
             To fix this, the merge_docker_onto_base function in chelsea_lib needs to be\n\
             updated to properly handle the merged-usr filesystem layout used by modern\n\
             Ubuntu docker images.",
            vm_status, error_text
        );
    }
}
