//! Integration tests for base image actions using testcontainers.
//!
//! These tests verify the orchestrator's base image management functionality
//! against a real PostgreSQL database (via testcontainers).
//!
//! Run with:
//! ```bash
//! cargo test --package orchestrator --test base_images -- --nocapture
//! ```
//!
//! Note: Since tests use ActionTestEnv::with_env which manages its own async runtime,
//! these tests should be run with --test-threads=1 to avoid conflicts.

use orch_test::ActionTestEnv;
use orchestrator::{
    action::{
        self, CreateBaseImage, CreateBaseImageRequest, GetBaseImageStatus, ImageSourceRequest,
        ListBaseImages,
    },
    db::{
        ApiKeyEntity, ApiKeysRepository, BaseImageJobsRepository, BaseImagesRepository,
        ImageSource, generate_rbd_image_name,
    },
};
use uuid::Uuid;

/// Helper to get a valid API key entity from the test database
async fn get_test_api_key(env: &ActionTestEnv) -> ApiKeyEntity {
    let keys = env
        .db()
        .keys()
        .list_valid()
        .await
        .expect("Should list keys");
    keys.into_iter()
        .next()
        .expect("Test database should have at least one API key")
}

#[test]
fn test_create_base_image_success() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        let request = CreateBaseImageRequest {
            image_name: "test-ubuntu-24.04".to_string(),
            source: ImageSourceRequest::Docker {
                image_ref: "ubuntu:24.04".to_string(),
            },
            size_mib: 512,
            description: Some("Test image".to_string()),
        };

        // Create the base image
        let result = action::call(CreateBaseImage::new(api_key.clone(), request.clone())).await;

        match result {
            Ok(response) => {
                tracing::info!(
                    job_id = %response.job_id,
                    image_name = %response.image_name,
                    status = %response.status,
                    "CreateBaseImage succeeded"
                );

                assert_eq!(response.image_name, "test-ubuntu-24.04");
                assert_eq!(response.status, "pending");

                // Verify job was created in database
                let job_id: Uuid = response
                    .job_id
                    .parse()
                    .expect("Job ID should be valid UUID");
                let job = env
                    .db()
                    .base_image_jobs()
                    .get_by_id(job_id)
                    .await
                    .expect("DB query should succeed")
                    .expect("Job should exist");

                assert_eq!(job.image_name, "test-ubuntu-24.04");
                assert_eq!(job.owner_id, owner_id);
                assert_eq!(job.source.source_type(), "docker");
                assert_eq!(job.size_mib, 512);

                // Verify RBD image name was generated correctly
                let expected_rbd_name = generate_rbd_image_name(owner_id, "test-ubuntu-24.04");
                assert_eq!(job.rbd_image_name, expected_rbd_name);

                tracing::info!("✓ CreateBaseImage test passed");
            }
            Err(e) => {
                panic!("CreateBaseImage failed: {:?}", e);
            }
        }
    });
}

#[test]
fn test_create_base_image_duplicate_name_rejected() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        let request = CreateBaseImageRequest {
            image_name: "duplicate-test-image".to_string(),
            source: ImageSourceRequest::Docker {
                image_ref: "ubuntu:24.04".to_string(),
            },
            size_mib: 512,
            description: None,
        };

        // Create first image - should succeed
        let result1 = action::call(CreateBaseImage::new(api_key.clone(), request.clone())).await;
        assert!(result1.is_ok(), "First image creation should succeed");

        // Manually insert a completed image record to simulate completion
        let rbd_name = generate_rbd_image_name(owner_id, "duplicate-test-image");
        env.db()
            .base_images()
            .insert(
                "duplicate-test-image",
                &rbd_name,
                owner_id,
                false,
                &ImageSource::Docker {
                    image_ref: "ubuntu:24.04".to_string(),
                },
                512,
                None,
            )
            .await
            .expect("Should insert image");

        // Try to create second image with same name - should fail
        let request2 = CreateBaseImageRequest {
            image_name: "duplicate-test-image".to_string(),
            source: ImageSourceRequest::Docker {
                image_ref: "debian:12".to_string(),
            },
            size_mib: 1024,
            description: None,
        };

        let result2 = action::call(CreateBaseImage::new(api_key.clone(), request2)).await;

        match result2 {
            Ok(_) => panic!("Should have rejected duplicate image name"),
            Err(e) => {
                let err = e.try_extract_err().expect("Should have error");
                tracing::info!("✓ Duplicate name correctly rejected: {:?}", err);
            }
        }
    });
}

#[test]
fn test_list_base_images_empty() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;

        let result = action::call(ListBaseImages::new(api_key, None, None)).await;

        match result {
            Ok(response) => {
                tracing::info!(
                    count = response.images.len(),
                    "ListBaseImages returned {} images",
                    response.images.len()
                );
                // Fresh database may have no images, or may have seed data
                // Just verify the call succeeds
                tracing::info!("✓ ListBaseImages test passed");
            }
            Err(e) => {
                panic!("ListBaseImages failed: {:?}", e);
            }
        }
    });
}

#[test]
fn test_list_base_images_with_data() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        // Insert some test images
        let rbd_name1 = generate_rbd_image_name(owner_id, "list-test-image-1");
        let rbd_name2 = generate_rbd_image_name(owner_id, "list-test-image-2");

        env.db()
            .base_images()
            .insert(
                "list-test-image-1",
                &rbd_name1,
                owner_id,
                false,
                &ImageSource::Docker {
                    image_ref: "ubuntu:24.04".to_string(),
                },
                512,
                Some("First test image"),
            )
            .await
            .expect("Should insert first image");

        env.db()
            .base_images()
            .insert(
                "list-test-image-2",
                &rbd_name2,
                owner_id,
                false,
                &ImageSource::Docker {
                    image_ref: "debian:12".to_string(),
                },
                1024,
                Some("Second test image"),
            )
            .await
            .expect("Should insert second image");

        let result = action::call(ListBaseImages::new(api_key, None, None)).await;

        match result {
            Ok(response) => {
                tracing::info!(
                    count = response.images.len(),
                    "ListBaseImages returned {} images",
                    response.images.len()
                );

                // Should have at least our 2 test images
                assert!(response.images.len() >= 2, "Should have at least 2 images");

                // Check that our images are in the list
                let names: Vec<_> = response.images.iter().map(|i| &i.image_name).collect();
                assert!(
                    names.contains(&&"list-test-image-1".to_string()),
                    "Should contain list-test-image-1"
                );
                assert!(
                    names.contains(&&"list-test-image-2".to_string()),
                    "Should contain list-test-image-2"
                );

                tracing::info!("✓ ListBaseImages with data test passed");
            }
            Err(e) => {
                panic!("ListBaseImages failed: {:?}", e);
            }
        }
    });
}

#[test]
fn test_get_base_image_status_completed() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        // Insert a completed image
        let rbd_name = generate_rbd_image_name(owner_id, "status-test-completed");
        env.db()
            .base_images()
            .insert(
                "status-test-completed",
                &rbd_name,
                owner_id,
                false,
                &ImageSource::Docker {
                    image_ref: "ubuntu:24.04".to_string(),
                },
                512,
                None,
            )
            .await
            .expect("Should insert image");

        let result = action::call(GetBaseImageStatus::new(
            api_key,
            "status-test-completed".to_string(),
        ))
        .await;

        match result {
            Ok(response) => {
                assert_eq!(response.image_name, "status-test-completed");
                assert_eq!(response.status, "completed");
                assert_eq!(response.size_mib, 512);
                assert!(response.error_message.is_none());

                tracing::info!("✓ GetBaseImageStatus (completed) test passed");
            }
            Err(e) => {
                panic!("GetBaseImageStatus failed: {:?}", e);
            }
        }
    });
}

#[test]
fn test_get_base_image_status_pending_job() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        // Create a pending job (no completed image)
        let rbd_name = generate_rbd_image_name(owner_id, "status-test-pending");
        env.db()
            .base_image_jobs()
            .insert(
                "status-test-pending",
                &rbd_name,
                owner_id,
                &ImageSource::Docker {
                    image_ref: "ubuntu:24.04".to_string(),
                },
                512,
            )
            .await
            .expect("Should insert job");

        let result = action::call(GetBaseImageStatus::new(
            api_key,
            "status-test-pending".to_string(),
        ))
        .await;

        match result {
            Ok(response) => {
                assert_eq!(response.image_name, "status-test-pending");
                assert_eq!(response.status, "pending");
                assert_eq!(response.size_mib, 512);

                tracing::info!("✓ GetBaseImageStatus (pending) test passed");
            }
            Err(e) => {
                panic!("GetBaseImageStatus failed: {:?}", e);
            }
        }
    });
}

#[test]
fn test_get_base_image_status_not_found() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;

        let result = action::call(GetBaseImageStatus::new(
            api_key,
            "nonexistent-image-12345".to_string(),
        ))
        .await;

        match result {
            Ok(_) => panic!("Should have returned NotFound error"),
            Err(e) => {
                let err = e.try_extract_err().expect("Should have error");
                tracing::info!(
                    "✓ GetBaseImageStatus correctly returned NotFound: {:?}",
                    err
                );
            }
        }
    });
}

#[test]
fn test_per_owner_namespacing() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        // Create two different RBD names for the same image name but different owners
        let other_owner_id = Uuid::new_v4();

        let rbd_name_owner1 = generate_rbd_image_name(owner_id, "shared-name");
        let rbd_name_owner2 = generate_rbd_image_name(other_owner_id, "shared-name");

        // The RBD names should be different
        assert_ne!(
            rbd_name_owner1, rbd_name_owner2,
            "Different owners should get different RBD names for the same image name"
        );

        tracing::info!(
            owner1_rbd = %rbd_name_owner1,
            owner2_rbd = %rbd_name_owner2,
            "✓ Per-owner namespacing works correctly"
        );
    });
}

#[test]
fn test_create_image_with_s3_source() {
    ActionTestEnv::with_env(|env| async move {
        let api_key = get_test_api_key(env).await;

        let request = CreateBaseImageRequest {
            image_name: "test-s3-image".to_string(),
            source: ImageSourceRequest::S3 {
                bucket: "my-bucket".to_string(),
                key: "images/rootfs.tar".to_string(),
            },
            size_mib: 1024,
            description: Some("Image from S3".to_string()),
        };

        let result = action::call(CreateBaseImage::new(api_key, request)).await;

        match result {
            Ok(response) => {
                assert_eq!(response.image_name, "test-s3-image");

                // Verify the job has correct source type
                let job_id: Uuid = response.job_id.parse().expect("Valid UUID");
                let job = env
                    .db()
                    .base_image_jobs()
                    .get_by_id(job_id)
                    .await
                    .expect("DB query should succeed")
                    .expect("Job should exist");

                assert_eq!(job.source.source_type(), "s3");
                match &job.source {
                    ImageSource::S3 { bucket, key } => {
                        assert_eq!(bucket, "my-bucket");
                        assert_eq!(key, "images/rootfs.tar");
                    }
                    _ => panic!("Expected S3 source"),
                }

                tracing::info!("✓ S3 source image creation test passed");
            }
            Err(e) => {
                panic!("CreateBaseImage with S3 source failed: {:?}", e);
            }
        }
    });
}
