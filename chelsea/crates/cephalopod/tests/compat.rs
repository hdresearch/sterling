//! Tests for the compat layer — verifies it's a drop-in for the old ceph crate API.
//!
//! These tests use the exact same type names and method signatures that
//! consumers of the old `ceph` crate use, just imported from `cephalopod::compat`.

use cephalopod::RbdSnapName;
use cephalopod::compat::{RbdClient, RbdClientError, RbdImageInfo, default_rbd_client};
use std::time::Duration;

fn test_name(suffix: &str) -> String {
    format!("cephalopod-compat-{}-{suffix}", std::process::id())
}

// ---------------------------------------------------------------------------
// RbdClient construction
// ---------------------------------------------------------------------------

#[test]
fn test_rbd_client_new() {
    let _client = RbdClient::new(
        "chelsea".to_string(),
        "rbd".to_string(),
        Duration::from_secs(30),
    )
    .expect("RbdClient::new");
}

#[test]
fn test_rbd_client_new_bad_user() {
    let err = RbdClient::new(
        "nonexistent_xyz".to_string(),
        "rbd".to_string(),
        Duration::from_secs(30),
    );
    assert!(err.is_err());
}

// ---------------------------------------------------------------------------
// default_rbd_client() singleton
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_default_rbd_client() {
    let client = default_rbd_client().expect("default_rbd_client");
    // Should work for basic operations
    let exists = client
        .image_exists("compat-nonexistent")
        .await
        .expect("exists");
    assert!(!exists);
}

#[tokio::test]
async fn test_default_rbd_client_same_instance() {
    let c1 = default_rbd_client().expect("1");
    let c2 = default_rbd_client().expect("2");
    assert!(std::ptr::eq(c1, c2));
}

// ---------------------------------------------------------------------------
// Image CRUD — same API as old ceph::RbdClient
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_image_lifecycle() {
    let client = default_rbd_client().expect("client");
    let name = test_name("lifecycle");

    client.image_create(&name, 64).await.expect("create");
    assert!(client.image_exists(&name).await.expect("exists"));

    let info: RbdImageInfo = client.image_info(&name).await.expect("info");
    assert_eq!(info.size_mib(), 64);
    assert_eq!(info.size, 64 * 1024 * 1024);

    let images = client.image_list().await.expect("list");
    assert!(images.contains(&name));

    client.image_remove(&name).await.expect("remove");
    assert!(
        !client
            .image_exists(&name)
            .await
            .expect("exists after remove")
    );
}

#[tokio::test]
async fn test_image_info_snapshot_count() {
    let client = default_rbd_client().expect("client");
    let name = test_name("snapcount");

    client.image_create(&name, 32).await.expect("create");

    let snap = RbdSnapName {
        image_name: name.clone(),
        snap_name: "s1".to_string(),
    };
    client.snap_create(&snap).await.expect("snap_create");

    let info = client.image_info(&name).await.expect("info");
    assert_eq!(info.snapshot_count, 1);

    client.snap_remove(&snap).await.expect("snap_remove");
    client.image_remove(&name).await.expect("remove");
}

#[tokio::test]
async fn test_image_remove_nonexistent() {
    let client = default_rbd_client().expect("client");
    let err = client.image_remove("compat-nonexistent-xyz").await;
    assert!(
        matches!(err, Err(RbdClientError::NotFound(_))),
        "expected NotFound, got: {err:?}"
    );
}

#[tokio::test]
async fn test_image_grow() {
    let client = default_rbd_client().expect("client");
    let name = test_name("grow");

    client.image_create(&name, 64).await.expect("create");
    client.image_grow(&name, 128).await.expect("grow");

    let info = client.image_info(&name).await.expect("info");
    assert_eq!(info.size_mib(), 128);

    client.image_remove(&name).await.expect("remove");
}

#[tokio::test]
async fn test_image_has_watchers() {
    let client = default_rbd_client().expect("client");
    let name = test_name("watchers");

    client.image_create(&name, 32).await.expect("create");
    let _ = client
        .image_has_watchers(&name)
        .await
        .expect("has_watchers");
    client.image_remove(&name).await.expect("remove");
}

// ---------------------------------------------------------------------------
// Snapshot operations — using &RbdSnapName like the old API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_snap_lifecycle() {
    let client = default_rbd_client().expect("client");
    let img = test_name("snap-life");

    client.image_create(&img, 32).await.expect("create");

    let snap = RbdSnapName {
        image_name: img.clone(),
        snap_name: "s1".to_string(),
    };
    client.snap_create(&snap).await.expect("snap_create");

    let snaps = client.snap_list(&img).await.expect("snap_list");
    assert_eq!(snaps.len(), 1);
    assert_eq!(snaps[0].image_name, img);
    assert_eq!(snaps[0].snap_name, "s1");

    client.snap_remove(&snap).await.expect("snap_remove");
    let snaps = client
        .snap_list(&img)
        .await
        .expect("snap_list after remove");
    assert!(snaps.is_empty());

    client.image_remove(&img).await.expect("remove");
}

#[tokio::test]
async fn test_snap_protect_clone_children() {
    let client = default_rbd_client().expect("client");
    let parent = test_name("compat-parent");
    let child = test_name("compat-child");

    client.image_create(&parent, 32).await.expect("create");

    let snap = RbdSnapName {
        image_name: parent.clone(),
        snap_name: "s1".to_string(),
    };
    client.snap_create(&snap).await.expect("snap_create");
    client.snap_protect(&snap).await.expect("protect");

    client.snap_clone(&snap, &child).await.expect("clone");
    assert!(client.image_exists(&child).await.expect("child exists"));
    assert!(client.snap_has_children(&snap).await.expect("has_children"));

    // Cleanup
    client.image_remove(&child).await.expect("remove child");
    client.snap_unprotect(&snap).await.expect("unprotect");
    client.snap_remove(&snap).await.expect("snap_remove");
    client.image_remove(&parent).await.expect("remove parent");
}

#[tokio::test]
async fn test_snap_purge() {
    let client = default_rbd_client().expect("client");
    let img = test_name("compat-purge");

    client.image_create(&img, 32).await.expect("create");

    let s1 = RbdSnapName {
        image_name: img.clone(),
        snap_name: "a".to_string(),
    };
    let s2 = RbdSnapName {
        image_name: img.clone(),
        snap_name: "b".to_string(),
    };
    client.snap_create(&s1).await.expect("snap a");
    client.snap_create(&s2).await.expect("snap b");

    client.snap_purge(&img).await.expect("purge");
    let snaps = client.snap_list(&img).await.expect("list");
    assert!(snaps.is_empty());

    client.image_remove(&img).await.expect("remove");
}

// ---------------------------------------------------------------------------
// Namespace operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_namespace_ensure() {
    let client = default_rbd_client().expect("client");
    let ns = format!("compat-ns-{}", std::process::id());

    client.namespace_ensure(&ns).await.expect("ensure");
    assert!(client.namespace_exists(&ns).await.expect("exists"));
    // Idempotent
    client.namespace_ensure(&ns).await.expect("ensure again");
}

// ---------------------------------------------------------------------------
// Error type compat — pattern matching that consumers do
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_error_not_found_pattern() {
    let client = default_rbd_client().expect("client");
    let err = client.image_remove("compat-nonexistent").await;

    match err {
        Err(RbdClientError::NotFound(msg)) => {
            assert!(!msg.is_empty());
        }
        other => panic!("expected NotFound, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_error_already_exists_pattern() {
    let client = default_rbd_client().expect("client");
    let name = test_name("compat-dup");

    client.image_create(&name, 32).await.expect("create");
    let err = client.image_create(&name, 32).await;

    // Old code matches on ExitCode with stderr containing "already exists"
    // Our compat layer maps AlreadyExists to ExitCode(17, ...)
    match err {
        Err(RbdClientError::ExitCode(17, _, _)) => {} // correct
        other => panic!("expected ExitCode(17, ...), got: {other:?}"),
    }

    client.image_remove(&name).await.expect("remove");
}

// ---------------------------------------------------------------------------
// Ceph diagnostic commands
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ceph_status() {
    let client = default_rbd_client().expect("client");
    let status = client.ceph_status().await.expect("ceph_status");
    assert!(status.contains("health:"));
}

#[tokio::test]
async fn test_ceph_client_version() {
    let client = default_rbd_client().expect("client");
    let v = client.ceph_client_version().await.expect("version");
    assert!(v.contains("ceph"));
}

#[tokio::test]
async fn test_ceph_cluster_version() {
    let client = default_rbd_client().expect("client");
    let v = client.ceph_cluster_version().await.expect("version");
    assert!(!v.is_empty());
}
