//! Integration tests for `vers_s3` backed by a MinIO testcontainer.
//!
//! Run with: `cargo nextest run -p vers_s3 --test transfer_integration`

use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::minio::MinIO;
use vers_s3::TransferConfig;

const MINIO_ACCESS_KEY: &str = "minioadmin";
const MINIO_SECRET_KEY: &str = "minioadmin";
const TEST_BUCKET: &str = "test-bucket";

async fn setup_minio() -> (ContainerAsync<MinIO>, Client) {
    let container = MinIO::default().start().await.unwrap();
    let port = container.get_host_port_ipv4(9000).await.unwrap();
    let endpoint = format!("http://127.0.0.1:{port}");

    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .endpoint_url(&endpoint)
        .credentials_provider(aws_sdk_s3::config::Credentials::new(
            MINIO_ACCESS_KEY,
            MINIO_SECRET_KEY,
            None,
            None,
            "test",
        ))
        .region(aws_sdk_s3::config::Region::new("us-east-1"))
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .force_path_style(true)
        .build();
    let client = Client::from_conf(s3_config);

    client
        .create_bucket()
        .bucket(TEST_BUCKET)
        .send()
        .await
        .unwrap();

    (container, client)
}

async fn put_object(client: &Client, key: &str, body: &[u8]) {
    client
        .put_object()
        .bucket(TEST_BUCKET)
        .key(key)
        .body(ByteStream::from(body.to_vec()))
        .send()
        .await
        .unwrap();
}

// ── get_file_size_bytes ──

#[tokio::test]
async fn get_file_size_bytes_returns_correct_size() {
    let (_c, client) = setup_minio().await;
    let data = vec![0u8; 1024 * 1024]; // 1 MiB
    put_object(&client, "sized.bin", &data).await;

    let size = vers_s3::get_file_size_bytes(&client, TEST_BUCKET, "sized.bin")
        .await
        .unwrap();

    assert_eq!(size, 1024 * 1024);
}

// ── download_file (single stream — small file) ──

#[tokio::test]
async fn download_small_file() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();
    let config = TransferConfig::default();

    let content = b"hello world small file";
    put_object(&client, "small.txt", content).await;

    let dst = dir.path().join("small.txt");
    vers_s3::download_file(&client, TEST_BUCKET, "small.txt", &dst, &config)
        .await
        .unwrap();

    assert_eq!(tokio::fs::read(&dst).await.unwrap(), content);
}

// ── download_file (parallel — large file) ──

#[tokio::test]
async fn download_large_file_parallel() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    // Use a tiny threshold and chunk size to force parallel path
    let config = TransferConfig {
        chunk_size: 256 * 1024, // 256 KiB chunks
        max_concurrency: 4,
        parallel_threshold: 512 * 1024, // 512 KiB threshold
    };

    // Create a 2 MiB file with recognizable pattern
    let mut data = Vec::with_capacity(2 * 1024 * 1024);
    for i in 0u32..(2 * 1024 * 1024 / 4) {
        data.extend_from_slice(&i.to_le_bytes());
    }
    put_object(&client, "large.bin", &data).await;

    let dst = dir.path().join("large.bin");
    vers_s3::download_file(&client, TEST_BUCKET, "large.bin", &dst, &config)
        .await
        .unwrap();

    let downloaded = tokio::fs::read(&dst).await.unwrap();
    assert_eq!(downloaded.len(), data.len());
    assert_eq!(downloaded, data, "Downloaded data must match byte-for-byte");
}

// ── download_file (empty file) ──

#[tokio::test]
async fn download_empty_file() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();
    let config = TransferConfig::default();

    put_object(&client, "empty.bin", b"").await;

    let dst = dir.path().join("empty.bin");
    vers_s3::download_file(&client, TEST_BUCKET, "empty.bin", &dst, &config)
        .await
        .unwrap();

    assert_eq!(tokio::fs::read(&dst).await.unwrap().len(), 0);
}

// ── download_file creates parent dirs ──

#[tokio::test]
async fn download_creates_parent_dirs() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();
    let config = TransferConfig::default();

    put_object(&client, "nested.txt", b"data").await;

    let dst = dir.path().join("a/b/c/nested.txt");
    vers_s3::download_file(&client, TEST_BUCKET, "nested.txt", &dst, &config)
        .await
        .unwrap();

    assert!(dst.exists());
}

// ── download_to_vec ──

#[tokio::test]
async fn download_to_vec_returns_contents() {
    let (_c, client) = setup_minio().await;

    put_object(&client, "mem.txt", b"in memory").await;

    let data = vers_s3::download_to_vec(&client, TEST_BUCKET, "mem.txt")
        .await
        .unwrap();

    assert_eq!(data, b"in memory");
}

// ── upload_file (single stream — small file) ──

#[tokio::test]
async fn upload_small_file() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();
    let config = TransferConfig::default();

    let src = dir.path().join("upload_small.txt");
    tokio::fs::write(&src, b"small upload content")
        .await
        .unwrap();

    vers_s3::upload_file(&client, TEST_BUCKET, "uploaded_small.txt", &src, &config)
        .await
        .unwrap();

    // Verify by downloading
    let data = vers_s3::download_to_vec(&client, TEST_BUCKET, "uploaded_small.txt")
        .await
        .unwrap();
    assert_eq!(data, b"small upload content");
}

// ── upload_file (parallel — large file) ──

#[tokio::test]
async fn upload_large_file_parallel() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let config = TransferConfig {
        chunk_size: 5 * 1024 * 1024, // 5 MiB (S3 minimum part size)
        max_concurrency: 4,
        parallel_threshold: 512 * 1024,
    };

    // Create a ~6 MiB file to force multipart (2 parts)
    let mut data = Vec::with_capacity(6 * 1024 * 1024);
    for i in 0u32..(6 * 1024 * 1024 / 4) {
        data.extend_from_slice(&i.to_le_bytes());
    }

    let src = dir.path().join("large_upload.bin");
    tokio::fs::write(&src, &data).await.unwrap();

    vers_s3::upload_file(&client, TEST_BUCKET, "uploaded_large.bin", &src, &config)
        .await
        .unwrap();

    // Verify by downloading
    let downloaded = vers_s3::download_to_vec(&client, TEST_BUCKET, "uploaded_large.bin")
        .await
        .unwrap();
    assert_eq!(downloaded.len(), data.len());
    assert_eq!(downloaded, data);
}

// ── upload_file (empty file) ──

#[tokio::test]
async fn upload_empty_file() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();
    let config = TransferConfig::default();

    let src = dir.path().join("empty_upload.bin");
    tokio::fs::write(&src, b"").await.unwrap();

    vers_s3::upload_file(&client, TEST_BUCKET, "uploaded_empty.bin", &src, &config)
        .await
        .unwrap();

    let data = vers_s3::download_to_vec(&client, TEST_BUCKET, "uploaded_empty.bin")
        .await
        .unwrap();
    assert_eq!(data.len(), 0);
}

// ── round-trip: upload then parallel download ──

#[tokio::test]
async fn round_trip_upload_download() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let config = TransferConfig {
        chunk_size: 5 * 1024 * 1024,
        max_concurrency: 4,
        parallel_threshold: 512 * 1024,
    };

    // Create recognizable pattern
    let mut data = Vec::with_capacity(6 * 1024 * 1024);
    for i in 0u32..(6 * 1024 * 1024 / 4) {
        data.extend_from_slice(&i.to_le_bytes());
    }

    let src = dir.path().join("roundtrip_src.bin");
    tokio::fs::write(&src, &data).await.unwrap();

    vers_s3::upload_file(&client, TEST_BUCKET, "roundtrip.bin", &src, &config)
        .await
        .unwrap();

    let dst = dir.path().join("roundtrip_dst.bin");
    vers_s3::download_file(&client, TEST_BUCKET, "roundtrip.bin", &dst, &config)
        .await
        .unwrap();

    let downloaded = tokio::fs::read(&dst).await.unwrap();
    assert_eq!(downloaded, data);
}
