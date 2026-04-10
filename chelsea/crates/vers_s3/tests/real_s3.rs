//! Real S3 integration tests — these hit actual AWS.
//!
//! Run with: `cargo nextest run -p vers_s3 --test real_s3`
//!
//! These tests use the `vers-commits-dev--use1-az4--x-s3` bucket and clean
//! up after themselves. They require valid AWS credentials in the environment.

use aws_sdk_s3::Client;
use std::time::Instant;
use vers_s3::TransferConfig;

const TEST_BUCKET: &str = "vers-commits-dev--use1-az4--x-s3";
const TEST_PREFIX: &str = "vers_s3_test/";

async fn make_client() -> Client {
    let config = aws_config::load_from_env().await;
    Client::new(&config)
}

/// Clean up a test object. Best-effort, never fails the test.
async fn cleanup(client: &Client, key: &str) {
    let _ = client
        .delete_object()
        .bucket(TEST_BUCKET)
        .key(key)
        .send()
        .await;
}

fn test_key(name: &str) -> String {
    format!("{TEST_PREFIX}{name}")
}

// ── Download: small file (single-stream path) ──

#[tokio::test]
async fn download_small_file_single_stream() {
    let client = make_client().await;
    let key = test_key("download_small.bin");
    let config = TransferConfig::default();

    // Upload a small test object
    let data = vec![42u8; 1024]; // 1 KiB
    client
        .put_object()
        .bucket(TEST_BUCKET)
        .key(&key)
        .body(data.clone().into())
        .send()
        .await
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("small.bin");

    vers_s3::download_file(&client, TEST_BUCKET, &key, &dst, &config)
        .await
        .unwrap();

    let downloaded = tokio::fs::read(&dst).await.unwrap();
    assert_eq!(downloaded, data);

    cleanup(&client, &key).await;
}

// ── Download: large file (parallel path) ──

#[tokio::test]
async fn download_large_file_parallel() {
    let client = make_client().await;
    let key = test_key("download_large.bin");

    // Force parallel with small thresholds
    let config = TransferConfig {
        chunk_size: 5 * 1024 * 1024, // 5 MiB chunks
        max_concurrency: 4,
        parallel_threshold: 1024 * 1024, // 1 MiB threshold
    };

    // Create a 20 MiB file with a recognizable pattern
    let size = 20 * 1024 * 1024;
    let mut data = Vec::with_capacity(size);
    for i in 0u32..(size / 4) as u32 {
        data.extend_from_slice(&i.to_le_bytes());
    }

    // Upload using SDK multipart (file is > 5MB)
    client
        .put_object()
        .bucket(TEST_BUCKET)
        .key(&key)
        .body(data.clone().into())
        .send()
        .await
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("large.bin");

    let start = Instant::now();
    vers_s3::download_file(&client, TEST_BUCKET, &key, &dst, &config)
        .await
        .unwrap();
    let elapsed = start.elapsed();

    let downloaded = tokio::fs::read(&dst).await.unwrap();
    assert_eq!(downloaded.len(), data.len());
    assert_eq!(downloaded, data, "Downloaded data must match byte-for-byte");

    println!("Downloaded {} MiB in {:?}", size / 1024 / 1024, elapsed);

    cleanup(&client, &key).await;
}

// ── Upload: small file (single PUT) ──

#[tokio::test]
async fn upload_small_file() {
    let client = make_client().await;
    let key = test_key("upload_small.bin");
    let config = TransferConfig::default();

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("small.bin");
    let data = vec![99u8; 4096];
    tokio::fs::write(&src, &data).await.unwrap();

    vers_s3::upload_file(&client, TEST_BUCKET, &key, &src, &config)
        .await
        .unwrap();

    // Verify by downloading with SDK
    let resp = client
        .get_object()
        .bucket(TEST_BUCKET)
        .key(&key)
        .send()
        .await
        .unwrap();
    let bytes = resp.body.collect().await.unwrap().to_vec();
    assert_eq!(bytes, data);

    cleanup(&client, &key).await;
}

// ── Upload: large file (multipart) ──

#[tokio::test]
async fn upload_large_file_multipart() {
    let client = make_client().await;
    let key = test_key("upload_large.bin");

    let config = TransferConfig {
        chunk_size: 5 * 1024 * 1024, // 5 MiB (S3 minimum part size)
        max_concurrency: 4,
        parallel_threshold: 1024 * 1024, // 1 MiB
    };

    // 12 MiB file → 3 parts with 5 MiB chunks
    let size = 12 * 1024 * 1024;
    let mut data = Vec::with_capacity(size);
    for i in 0u32..(size / 4) as u32 {
        data.extend_from_slice(&i.to_le_bytes());
    }

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("large.bin");
    tokio::fs::write(&src, &data).await.unwrap();

    let start = Instant::now();
    vers_s3::upload_file(&client, TEST_BUCKET, &key, &src, &config)
        .await
        .unwrap();
    let elapsed = start.elapsed();

    // Verify
    let resp = client
        .get_object()
        .bucket(TEST_BUCKET)
        .key(&key)
        .send()
        .await
        .unwrap();
    let bytes = resp.body.collect().await.unwrap().to_vec();
    assert_eq!(bytes.len(), data.len());
    assert_eq!(bytes, data);

    println!("Uploaded {} MiB in {:?}", size / 1024 / 1024, elapsed);

    cleanup(&client, &key).await;
}

// ── Round-trip at realistic size ──

#[tokio::test]
async fn round_trip_realistic() {
    let client = make_client().await;
    let key = test_key("roundtrip.bin");

    let config = TransferConfig {
        chunk_size: 8 * 1024 * 1024, // 8 MiB
        max_concurrency: 8,
        parallel_threshold: 1024 * 1024,
    };

    // 32 MiB — small enough to not be slow in CI, big enough to exercise parallelism
    let size = 32 * 1024 * 1024;
    let mut data = Vec::with_capacity(size);
    for i in 0u32..(size / 4) as u32 {
        data.extend_from_slice(&i.to_le_bytes());
    }

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("up.bin");
    tokio::fs::write(&src, &data).await.unwrap();

    // Upload
    let up_start = Instant::now();
    vers_s3::upload_file(&client, TEST_BUCKET, &key, &src, &config)
        .await
        .unwrap();
    let up_elapsed = up_start.elapsed();

    // Download
    let dst = dir.path().join("down.bin");
    let down_start = Instant::now();
    vers_s3::download_file(&client, TEST_BUCKET, &key, &dst, &config)
        .await
        .unwrap();
    let down_elapsed = down_start.elapsed();

    let downloaded = tokio::fs::read(&dst).await.unwrap();
    assert_eq!(downloaded.len(), data.len());
    assert_eq!(downloaded, data);

    println!(
        "Round-trip {} MiB: upload {:?}, download {:?}",
        size / 1024 / 1024,
        up_elapsed,
        down_elapsed
    );

    cleanup(&client, &key).await;
}

// ── get_file_size_bytes ──

#[tokio::test]
async fn get_file_size_bytes_correct() {
    let client = make_client().await;
    let key = test_key("size_check.bin");

    let data = vec![0u8; 12345];
    client
        .put_object()
        .bucket(TEST_BUCKET)
        .key(&key)
        .body(data.into())
        .send()
        .await
        .unwrap();

    let size = vers_s3::get_file_size_bytes(&client, TEST_BUCKET, &key)
        .await
        .unwrap();
    assert_eq!(size, 12345);

    cleanup(&client, &key).await;
}

// ── download_to_vec ──

#[tokio::test]
async fn download_to_vec_works() {
    let client = make_client().await;
    let key = test_key("to_vec.txt");

    client
        .put_object()
        .bucket(TEST_BUCKET)
        .key(&key)
        .body(b"hello from s3".to_vec().into())
        .send()
        .await
        .unwrap();

    let data = vers_s3::download_to_vec(&client, TEST_BUCKET, &key)
        .await
        .unwrap();
    assert_eq!(data, b"hello from s3");

    cleanup(&client, &key).await;
}
