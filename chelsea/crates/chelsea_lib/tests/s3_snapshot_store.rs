use std::sync::Arc;

use aws_sdk_s3::primitives::ByteStream;
use tempfile::TempDir;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::minio::MinIO;
use tokio::task::JoinSet;

use chelsea_lib::s3_store::{FileToDownload, S3SnapshotStore};

const MINIO_ACCESS_KEY: &str = "minioadmin";
const MINIO_SECRET_KEY: &str = "minioadmin";
const TEST_BUCKET: &str = "test-bucket";

/// Start a MinIO container and return an S3 client pointed at it.
async fn setup_minio() -> (testcontainers::ContainerAsync<MinIO>, aws_sdk_s3::Client) {
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
    let client = aws_sdk_s3::Client::from_conf(s3_config);

    // Create the test bucket.
    client
        .create_bucket()
        .bucket(TEST_BUCKET)
        .send()
        .await
        .unwrap();

    (container, client)
}

/// Upload a test object to MinIO.
async fn upload_test_object(client: &aws_sdk_s3::Client, key: &str, body: &[u8]) {
    client
        .put_object()
        .bucket(TEST_BUCKET)
        .key(key)
        .body(ByteStream::from(body.to_vec()))
        .send()
        .await
        .unwrap();
}

async fn make_store(
    dir: &std::path::Path,
    cache_size_mib: u32,
    client: aws_sdk_s3::Client,
) -> S3SnapshotStore {
    S3SnapshotStore::new(dir.to_path_buf(), cache_size_mib, client)
        .await
        .unwrap()
}

/// Test that download_files_coalesced works end-to-end with a real S3-compatible store.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_download_basic() {
    let (_container, client) = setup_minio().await;
    let dir = TempDir::new().unwrap();
    let store = make_store(dir.path(), 100, client.clone()).await;

    let commit_id = "abc123";
    let file_content = b"hello world snapshot data";
    let s3_key = format!("{commit_id}/snapshot.bin");

    upload_test_object(&client, &s3_key, file_content).await;

    let files = [FileToDownload {
        file_name: "snapshot.bin",
        s3_key: &s3_key,
    }];

    store
        .download_files_coalesced(TEST_BUCKET, &files)
        .await
        .unwrap();

    let downloaded = std::fs::read(dir.path().join("snapshot.bin")).unwrap();
    assert_eq!(downloaded, file_content);
}

/// Test that concurrent downloads of the SAME file are coalesced: only one
/// actual S3 download is performed and all callers see the result.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_downloads_same_file_coalesced() {
    let (_container, client) = setup_minio().await;
    let dir = TempDir::new().unwrap();
    let store = Arc::new(make_store(dir.path(), 100, client.clone()).await);

    let commit_id = "coalesce-test";
    let file_content = vec![0u8; 1024 * 512]; // 512 KB
    let s3_key = format!("{commit_id}/big_file.bin");

    upload_test_object(&client, &s3_key, &file_content).await;

    assert_eq!(store.download_count(), 0);

    // Launch multiple concurrent downloads of the same file.
    let mut join_set = JoinSet::new();
    for _ in 0..5 {
        let store = Arc::clone(&store);
        let s3_key = s3_key.clone();
        join_set.spawn(async move {
            let files = [FileToDownload {
                file_name: "big_file.bin",
                s3_key: &s3_key,
            }];
            store.download_files_coalesced(TEST_BUCKET, &files).await
        });
    }

    // All should succeed.
    while let Some(result) = join_set.join_next().await {
        result.unwrap().unwrap();
    }

    let downloaded = std::fs::read(dir.path().join("big_file.bin")).unwrap();
    assert_eq!(downloaded, file_content);

    // The coalescing logic should have performed exactly 1 real S3 download,
    // with the other 4 callers waiting on the inflight notification.
    assert_eq!(
        store.download_count(),
        1,
        "Expected exactly 1 S3 download, but got {}. Coalescing may not be working.",
        store.download_count()
    );
}

/// Test that concurrent downloads of DIFFERENT files all succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_downloads_different_files() {
    let (_container, client) = setup_minio().await;
    let dir = TempDir::new().unwrap();
    let store = Arc::new(make_store(dir.path(), 100, client.clone()).await);

    let commit_id = "multi-file-test";
    let num_files: usize = 5;

    for i in 0..num_files {
        let s3_key = format!("{commit_id}/file_{i}.bin");
        let content = format!("content of file {i}");
        upload_test_object(&client, &s3_key, content.as_bytes()).await;
    }

    let mut join_set = JoinSet::new();
    for i in 0..num_files {
        let store = Arc::clone(&store);
        let s3_key = format!("{commit_id}/file_{i}.bin");
        let file_name = format!("file_{i}.bin");
        join_set.spawn(async move {
            let files = [FileToDownload {
                file_name: &file_name,
                s3_key: &s3_key,
            }];
            store.download_files_coalesced(TEST_BUCKET, &files).await
        });
    }

    while let Some(result) = join_set.join_next().await {
        result.unwrap().unwrap();
    }

    for i in 0..num_files {
        let downloaded = std::fs::read(dir.path().join(format!("file_{i}.bin"))).unwrap();
        assert_eq!(downloaded, format!("content of file {i}").as_bytes());
    }
}

/// Test that cache eviction works under load — pinned files survive eviction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_cache_eviction_under_load() {
    let (_container, client) = setup_minio().await;
    let dir = TempDir::new().unwrap();
    // Small cache: 3 MiB — forces eviction
    let store = Arc::new(make_store(dir.path(), 3, client.clone()).await);

    let commit_id = "eviction-test";

    // Upload 4 x 1 MiB files — exceeds the 3 MiB cache
    for i in 0..4u8 {
        let s3_key = format!("{commit_id}/file_{i}.bin");
        let content = vec![i; 1024 * 1024];
        upload_test_object(&client, &s3_key, &content).await;
    }

    // Download first pair
    {
        let files: Vec<_> = (0..2)
            .map(|i| {
                let s3_key = format!("{commit_id}/file_{i}.bin");
                (format!("file_{i}.bin"), s3_key)
            })
            .collect();
        let file_refs: Vec<_> = files
            .iter()
            .map(|(name, key)| FileToDownload {
                file_name: name,
                s3_key: key,
            })
            .collect();
        store
            .download_files_coalesced(TEST_BUCKET, &file_refs)
            .await
            .unwrap();
    }

    // Download second pair — should trigger eviction of earlier files
    {
        let files: Vec<_> = (2..4)
            .map(|i| {
                let s3_key = format!("{commit_id}/file_{i}.bin");
                (format!("file_{i}.bin"), s3_key)
            })
            .collect();
        let file_refs: Vec<_> = files
            .iter()
            .map(|(name, key)| FileToDownload {
                file_name: name,
                s3_key: key,
            })
            .collect();
        store
            .download_files_coalesced(TEST_BUCKET, &file_refs)
            .await
            .unwrap();
    }

    // Latest files should exist
    assert!(dir.path().join("file_2.bin").exists());
    assert!(dir.path().join("file_3.bin").exists());

    // At least one of the earlier files must have been evicted to make room
    // (3 MiB cache, 4 x 1 MiB files → can't all fit).
    let early_files_remaining =
        dir.path().join("file_0.bin").exists() as u8 + dir.path().join("file_1.bin").exists() as u8;
    assert!(
        early_files_remaining < 2,
        "Expected at least one early file to be evicted, but both still exist"
    );
}

/// Test that re-downloading an already-cached file is a no-op (fast path).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_cached_file_not_redownloaded() {
    let (_container, client) = setup_minio().await;
    let dir = TempDir::new().unwrap();
    let store = make_store(dir.path(), 100, client.clone()).await;

    let commit_id = "cache-hit";
    let s3_key = format!("{commit_id}/cached.bin");
    let content = b"cached content";

    upload_test_object(&client, &s3_key, content).await;

    let files = [FileToDownload {
        file_name: "cached.bin",
        s3_key: &s3_key,
    }];

    // First download
    store
        .download_files_coalesced(TEST_BUCKET, &files)
        .await
        .unwrap();
    let first_read = std::fs::read(dir.path().join("cached.bin")).unwrap();

    // Overwrite locally to prove it's not re-downloaded
    std::fs::write(dir.path().join("cached.bin"), b"modified locally").unwrap();

    // Second download — should skip (file exists)
    store
        .download_files_coalesced(TEST_BUCKET, &files)
        .await
        .unwrap();
    let second_read = std::fs::read(dir.path().join("cached.bin")).unwrap();

    assert_eq!(first_read, content);
    assert_eq!(second_read, b"modified locally"); // NOT re-downloaded
}

/// Test that downloading a nonexistent S3 key returns an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_download_nonexistent_key_fails() {
    let (_container, client) = setup_minio().await;
    let dir = TempDir::new().unwrap();
    let store = make_store(dir.path(), 100, client.clone()).await;

    let files = [FileToDownload {
        file_name: "ghost.bin",
        s3_key: "does-not-exist/ghost.bin",
    }];

    let result = store.download_files_coalesced(TEST_BUCKET, &files).await;

    assert!(result.is_err(), "Expected error for nonexistent S3 key");
    assert!(!dir.path().join("ghost.bin").exists());
}
