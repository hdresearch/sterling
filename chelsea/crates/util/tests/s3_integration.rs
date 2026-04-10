//! Integration tests for `util::s3` backed by a MinIO testcontainer.
//!
//! Run with: `cargo nextest run -p util --test s3_integration`

use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use std::path::Path;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::minio::MinIO;

const MINIO_ACCESS_KEY: &str = "minioadmin";
const MINIO_SECRET_KEY: &str = "minioadmin";
const TEST_BUCKET: &str = "test-bucket";

/// Start a MinIO container and return an S3 client pointed at it.
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

/// Helper: upload test data directly via the SDK.
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

// ────────────────────────────────────────────────────────────────────────────
// list_objects
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_objects_with_prefix_returns_matching_keys() {
    let (_c, client) = setup_minio().await;

    put_object(&client, "dir/a.txt", b"a").await;
    put_object(&client, "dir/b.txt", b"b").await;
    put_object(&client, "other/c.txt", b"c").await;

    let mut keys = util::s3::list_objects_with_prefix(&client, TEST_BUCKET, "dir/")
        .await
        .unwrap();
    keys.sort();

    assert_eq!(keys, vec!["dir/a.txt", "dir/b.txt"]);
}

#[tokio::test]
async fn list_objects_empty_prefix_returns_all() {
    let (_c, client) = setup_minio().await;

    put_object(&client, "x.txt", b"x").await;
    put_object(&client, "y.txt", b"y").await;

    let keys = util::s3::list_objects_with_prefix(&client, TEST_BUCKET, "")
        .await
        .unwrap();

    assert_eq!(keys.len(), 2);
}

#[tokio::test]
async fn list_objects_no_matches_returns_empty() {
    let (_c, client) = setup_minio().await;

    let keys = util::s3::list_objects_with_prefix(&client, TEST_BUCKET, "nonexistent/")
        .await
        .unwrap();

    assert!(keys.is_empty());
}

// ────────────────────────────────────────────────────────────────────────────
// read_file / download_file
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn read_file_from_s3_returns_contents() {
    let (_c, client) = setup_minio().await;

    put_object(&client, "hello.txt", b"hello world").await;

    let data = util::s3::read_file_from_s3(&client, TEST_BUCKET, "hello.txt")
        .await
        .unwrap();

    assert_eq!(data, b"hello world");
}

#[tokio::test]
async fn download_file_from_s3_writes_to_disk() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let content = b"downloaded content";
    put_object(&client, "dl/file.bin", content).await;

    let dest = dir.path().join("file.bin");
    util::s3::download_file_from_s3(&client, TEST_BUCKET, "dl/file.bin", &dest)
        .await
        .unwrap();

    assert_eq!(tokio::fs::read(&dest).await.unwrap(), content);
}

#[tokio::test]
async fn download_file_creates_parent_dirs() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    put_object(&client, "nested/file.bin", b"data").await;

    let dest = dir.path().join("a/b/c/file.bin");
    util::s3::download_file_from_s3(&client, TEST_BUCKET, "nested/file.bin", &dest)
        .await
        .unwrap();

    assert!(dest.exists());
}

// ────────────────────────────────────────────────────────────────────────────
// get_s3_file_size_mib / get_total_s3_file_size_mib_many
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_s3_file_size_mib_rounds_up() {
    let (_c, client) = setup_minio().await;

    // Upload 100 bytes — should round up to 1 MiB
    put_object(&client, "tiny.bin", &[0u8; 100]).await;

    let size = util::s3::get_s3_file_size_mib(&client, TEST_BUCKET, "tiny.bin")
        .await
        .unwrap();

    assert_eq!(size, 1);
}

#[tokio::test]
async fn get_s3_file_size_mib_exact_mib() {
    let (_c, client) = setup_minio().await;

    // Upload exactly 1 MiB
    put_object(&client, "exact.bin", &vec![0u8; 1024 * 1024]).await;

    let size = util::s3::get_s3_file_size_mib(&client, TEST_BUCKET, "exact.bin")
        .await
        .unwrap();

    assert_eq!(size, 1);
}

#[tokio::test]
async fn get_total_s3_file_size_mib_many_sums_correctly() {
    let (_c, client) = setup_minio().await;

    // Two files: 100 bytes each → 1 MiB each → total 2 MiB
    put_object(&client, "size/a.bin", &[0u8; 100]).await;
    put_object(&client, "size/b.bin", &[0u8; 100]).await;

    let total = util::s3::get_total_s3_file_size_mib_many(
        &client,
        TEST_BUCKET,
        ["size/a.bin", "size/b.bin"],
    )
    .await
    .unwrap();

    assert_eq!(total, 2);
}

// ────────────────────────────────────────────────────────────────────────────
// delete_object / delete_objects
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_object_removes_key() {
    let (_c, client) = setup_minio().await;

    put_object(&client, "to-delete.txt", b"bye").await;

    util::s3::delete_object(&client, TEST_BUCKET, "to-delete.txt")
        .await
        .unwrap();

    let keys = util::s3::list_objects_with_prefix(&client, TEST_BUCKET, "to-delete.txt")
        .await
        .unwrap();
    assert!(keys.is_empty());
}

#[tokio::test]
async fn delete_objects_removes_multiple_keys() {
    let (_c, client) = setup_minio().await;

    put_object(&client, "batch/a.txt", b"a").await;
    put_object(&client, "batch/b.txt", b"b").await;
    put_object(&client, "batch/c.txt", b"c").await;

    util::s3::delete_objects(&client, TEST_BUCKET, ["batch/a.txt", "batch/b.txt"])
        .await
        .unwrap();

    let keys = util::s3::list_objects_with_prefix(&client, TEST_BUCKET, "batch/")
        .await
        .unwrap();
    assert_eq!(keys, vec!["batch/c.txt"]);
}

// ────────────────────────────────────────────────────────────────────────────
// compare_checksums
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn compare_checksums_matching() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let checksum_data = b"same checksum bytes";

    // Write the same checksum locally and to S3
    let local_path = dir.path().join("file.sha512");
    tokio::fs::write(&local_path, checksum_data).await.unwrap();
    put_object(&client, "ck/file.sha512", checksum_data).await;

    let result = util::s3::compare_checksums(&client, TEST_BUCKET, &local_path, "ck/file.sha512")
        .await
        .unwrap();

    assert!(result, "identical checksums should return true");
}

#[tokio::test]
async fn compare_checksums_different() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let local_path = dir.path().join("file.sha512");
    tokio::fs::write(&local_path, b"local checksum")
        .await
        .unwrap();
    put_object(&client, "ck/file.sha512", b"remote checksum").await;

    let result = util::s3::compare_checksums(&client, TEST_BUCKET, &local_path, "ck/file.sha512")
        .await
        .unwrap();

    assert!(!result, "different checksums should return false");
}

#[tokio::test]
async fn compare_checksums_missing_local_file() {
    let (_c, client) = setup_minio().await;

    put_object(&client, "ck/remote.sha512", b"data").await;

    let result = util::s3::compare_checksums(
        &client,
        TEST_BUCKET,
        Path::new("/tmp/nonexistent_checksum_file"),
        "ck/remote.sha512",
    )
    .await
    .unwrap();

    assert!(!result, "missing local file should return false");
}

// ────────────────────────────────────────────────────────────────────────────
// download_directory_from_s3
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn download_directory_from_s3_gets_all_files() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    put_object(&client, "mydir/one.txt", b"one").await;
    put_object(&client, "mydir/two.txt", b"two").await;
    put_object(&client, "mydir/three.txt", b"three").await;

    util::s3::download_directory_from_s3(&client, TEST_BUCKET, "mydir/", dir.path())
        .await
        .unwrap();

    assert_eq!(
        tokio::fs::read(dir.path().join("one.txt")).await.unwrap(),
        b"one"
    );
    assert_eq!(
        tokio::fs::read(dir.path().join("two.txt")).await.unwrap(),
        b"two"
    );
    assert_eq!(
        tokio::fs::read(dir.path().join("three.txt")).await.unwrap(),
        b"three"
    );
}

#[tokio::test]
async fn download_directory_creates_output_dir() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("new_dir");

    put_object(&client, "auto/file.txt", b"auto").await;

    util::s3::download_directory_from_s3(&client, TEST_BUCKET, "auto/", &out)
        .await
        .unwrap();

    assert!(out.join("file.txt").exists());
}

#[tokio::test]
async fn download_directory_empty_prefix() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    // No objects with this prefix
    util::s3::download_directory_from_s3(&client, TEST_BUCKET, "empty_prefix/", dir.path())
        .await
        .unwrap();

    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    assert!(entries.next_entry().await.unwrap().is_none());
}

// ────────────────────────────────────────────────────────────────────────────
// download_from_s3_directory_if_checksums_differ
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn checksum_diff_download_fetches_missing_files() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let content = b"file content";

    // Compute real sha512 checksum
    use sha2::{Digest, Sha512};
    let checksum = Sha512::digest(content);

    put_object(&client, "ckdir/data.bin", content).await;
    put_object(&client, "ckdir/data.bin.sha512", &checksum).await;

    util::s3::download_from_s3_directory_if_checksums_differ(
        &client,
        TEST_BUCKET,
        "ckdir/",
        dir.path(),
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read(dir.path().join("data.bin")).await.unwrap(),
        content
    );
    assert!(dir.path().join("data.bin.sha512").exists());
}

#[tokio::test]
async fn checksum_diff_skips_matching_files() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let content = b"existing content";

    use sha2::{Digest, Sha512};
    let checksum = Sha512::digest(content);

    put_object(&client, "ckskip/data.bin", content).await;
    put_object(&client, "ckskip/data.bin.sha512", &checksum).await;

    // Pre-populate local files with matching content and checksum
    tokio::fs::write(dir.path().join("data.bin"), content)
        .await
        .unwrap();
    tokio::fs::write(dir.path().join("data.bin.sha512"), &checksum[..])
        .await
        .unwrap();

    // Overwrite local file with a marker to prove it's NOT re-downloaded
    tokio::fs::write(dir.path().join("data.bin"), b"local marker")
        .await
        .unwrap();

    util::s3::download_from_s3_directory_if_checksums_differ(
        &client,
        TEST_BUCKET,
        "ckskip/",
        dir.path(),
    )
    .await
    .unwrap();

    // File should still have our local marker — checksums matched so it was skipped
    assert_eq!(
        tokio::fs::read(dir.path().join("data.bin")).await.unwrap(),
        b"local marker"
    );
}

#[tokio::test]
async fn checksum_diff_redownloads_on_mismatch() {
    let (_c, client) = setup_minio().await;
    let dir = tempfile::tempdir().unwrap();

    let remote_content = b"remote version";

    use sha2::{Digest, Sha512};
    let remote_checksum = Sha512::digest(remote_content);

    put_object(&client, "ckmis/data.bin", remote_content).await;
    put_object(&client, "ckmis/data.bin.sha512", &remote_checksum).await;

    // Pre-populate local files with DIFFERENT content and old checksum
    tokio::fs::write(dir.path().join("data.bin"), b"old version")
        .await
        .unwrap();
    let old_checksum = Sha512::digest(b"old version");
    tokio::fs::write(dir.path().join("data.bin.sha512"), &old_checksum[..])
        .await
        .unwrap();

    util::s3::download_from_s3_directory_if_checksums_differ(
        &client,
        TEST_BUCKET,
        "ckmis/",
        dir.path(),
    )
    .await
    .unwrap();

    // File should have been re-downloaded
    assert_eq!(
        tokio::fs::read(dir.path().join("data.bin")).await.unwrap(),
        remote_content
    );
}
