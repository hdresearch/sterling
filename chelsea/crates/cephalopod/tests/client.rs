//! Integration tests for the async Client.
//! Requires a running Ceph cluster with "chelsea" client and "rbd" pool.
//!
//! Run with: cargo nextest run -p cephalopod

use cephalopod::{CephalopodError, Client, RbdSnapName};

// nix is used for geteuid() to check root in device tests
extern crate nix;

fn test_name(suffix: &str) -> String {
    format!("cephalopod-client-{}-{suffix}", std::process::id())
}

fn client() -> Client {
    Client::connect("chelsea", "rbd").expect("connect")
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_connect() {
    let _c = client();
}

#[tokio::test]
async fn test_connect_bad_user() {
    let err = Client::connect("nonexistent_user_xyz", "rbd");
    assert!(err.is_err());
}

// ---------------------------------------------------------------------------
// Image CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_image_lifecycle() {
    let c = client();
    let name = test_name("lifecycle");

    c.image_create(&name, 64).await.expect("create");
    assert!(c.image_exists(&name).await.expect("exists"));

    let info = c.image_info(&name).await.expect("info");
    assert_eq!(info.size_mib(), 64);

    let images = c.image_list().await.expect("list");
    assert!(images.contains(&name));

    c.image_remove(&name).await.expect("remove");
    assert!(!c.image_exists(&name).await.expect("exists after remove"));
}

#[tokio::test]
async fn test_image_exists_nonexistent() {
    let c = client();
    assert!(
        !c.image_exists("cephalopod-client-nonexistent")
            .await
            .expect("exists")
    );
}

#[tokio::test]
async fn test_image_create_duplicate() {
    let c = client();
    let name = test_name("dup");

    c.image_create(&name, 32).await.expect("create");
    let err = c.image_create(&name, 32).await;
    assert!(
        matches!(err, Err(CephalopodError::AlreadyExists(_))),
        "got: {err:?}"
    );

    c.image_remove(&name).await.expect("remove");
}

#[tokio::test]
async fn test_image_remove_nonexistent() {
    let c = client();
    let err = c.image_remove("cephalopod-client-nonexistent").await;
    assert!(
        matches!(err, Err(CephalopodError::NotFound(_))),
        "got: {err:?}"
    );
}

#[tokio::test]
async fn test_image_grow() {
    let c = client();
    let name = test_name("grow");

    c.image_create(&name, 64).await.expect("create");
    c.image_grow(&name, 128).await.expect("grow");

    let info = c.image_info(&name).await.expect("info");
    assert_eq!(info.size_mib(), 128);

    c.image_remove(&name).await.expect("remove");
}

#[tokio::test]
async fn test_image_grow_same_size_noop() {
    let c = client();
    let name = test_name("grow-noop");

    c.image_create(&name, 64).await.expect("create");
    c.image_grow(&name, 64).await.expect("grow same size");

    let info = c.image_info(&name).await.expect("info");
    assert_eq!(info.size_mib(), 64);

    c.image_remove(&name).await.expect("remove");
}

#[tokio::test]
async fn test_image_has_watchers_unmapped() {
    let c = client();
    let name = test_name("watchers");

    c.image_create(&name, 32).await.expect("create");
    // Just verify it doesn't error — the open-for-stat may register a watcher
    let _ = c.image_has_watchers(&name).await.expect("has_watchers");
    c.image_remove(&name).await.expect("remove");
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_snap_lifecycle() {
    let c = client();
    let img = test_name("snap-life");

    c.image_create(&img, 32).await.expect("create");
    c.snap_create(&img, "s1").await.expect("snap_create");

    let snaps = c.snap_list(&img).await.expect("snap_list");
    assert_eq!(snaps.len(), 1);
    assert_eq!(snaps[0].name, "s1");

    c.snap_remove(&img, "s1").await.expect("snap_remove");
    let snaps = c.snap_list(&img).await.expect("snap_list after remove");
    assert!(snaps.is_empty());

    c.image_remove(&img).await.expect("remove");
}

#[tokio::test]
async fn test_snap_protect_unprotect() {
    let c = client();
    let img = test_name("snap-prot");

    c.image_create(&img, 32).await.expect("create");
    c.snap_create(&img, "s1").await.expect("snap_create");

    c.snap_protect(&img, "s1").await.expect("protect");
    // Can't remove while protected
    assert!(c.snap_remove(&img, "s1").await.is_err());

    c.snap_unprotect(&img, "s1").await.expect("unprotect");
    c.snap_remove(&img, "s1").await.expect("snap_remove");
    c.image_remove(&img).await.expect("remove");
}

#[tokio::test]
async fn test_snap_purge() {
    let c = client();
    let img = test_name("snap-purge");

    c.image_create(&img, 32).await.expect("create");
    c.snap_create(&img, "a").await.expect("snap a");
    c.snap_create(&img, "b").await.expect("snap b");

    c.snap_purge(&img).await.expect("purge");
    let snaps = c.snap_list(&img).await.expect("snap_list");
    assert!(snaps.is_empty());

    c.image_remove(&img).await.expect("remove");
}

#[tokio::test]
async fn test_snap_clone_and_children() {
    let c = client();
    let parent = test_name("clone-parent");
    let child = test_name("clone-child");

    c.image_create(&parent, 32).await.expect("create parent");
    c.snap_create(&parent, "s1").await.expect("snap_create");
    c.snap_protect(&parent, "s1").await.expect("protect");

    c.snap_clone(&parent, "s1", &child).await.expect("clone");
    assert!(c.image_exists(&child).await.expect("child exists"));

    assert!(
        c.snap_has_children(&parent, "s1")
            .await
            .expect("has_children")
    );

    let children = c
        .snap_list_children(&parent, "s1")
        .await
        .expect("list_children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].image_name, child);

    // Cleanup
    c.image_remove(&child).await.expect("remove child");
    c.snap_unprotect(&parent, "s1").await.expect("unprotect");
    c.snap_remove(&parent, "s1").await.expect("snap_remove");
    c.image_remove(&parent).await.expect("remove parent");
}

#[tokio::test]
async fn test_snap_has_children_empty() {
    let c = client();
    let img = test_name("no-children");

    c.image_create(&img, 32).await.expect("create");
    c.snap_create(&img, "s1").await.expect("snap_create");
    c.snap_protect(&img, "s1").await.expect("protect");

    assert!(!c.snap_has_children(&img, "s1").await.expect("has_children"));

    c.snap_unprotect(&img, "s1").await.expect("unprotect");
    c.snap_remove(&img, "s1").await.expect("snap_remove");
    c.image_remove(&img).await.expect("remove");
}

// ---------------------------------------------------------------------------
// Namespaces
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_namespace_lifecycle() {
    let c = client();
    let ns = format!("cephalopod-client-ns-{}", std::process::id());

    assert!(!c.namespace_exists(&ns).await.expect("exists before"));
    c.namespace_create(&ns).await.expect("create");
    assert!(c.namespace_exists(&ns).await.expect("exists after"));

    let list = c.namespace_list().await.expect("list");
    assert!(list.contains(&ns));
}

#[tokio::test]
async fn test_namespace_ensure_idempotent() {
    let c = client();
    let ns = format!("cephalopod-client-ensure-{}", std::process::id());

    c.namespace_ensure(&ns).await.expect("ensure 1");
    c.namespace_ensure(&ns)
        .await
        .expect("ensure 2 (idempotent)");
    assert!(c.namespace_exists(&ns).await.expect("exists"));
}

// ---------------------------------------------------------------------------
// Namespace-scoped images (namespaced image names)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_namespaced_image_operations() {
    let c = client();
    let ns = format!("cephalopod-client-nsimg-{}", std::process::id());
    c.namespace_ensure(&ns).await.expect("ensure ns");

    let namespaced_name = format!("{ns}/test-image");

    c.image_create(&namespaced_name, 32)
        .await
        .expect("create in ns");
    assert!(
        c.image_exists(&namespaced_name)
            .await
            .expect("exists in ns")
    );

    let info = c.image_info(&namespaced_name).await.expect("info");
    assert_eq!(info.size_mib(), 32);

    // Should NOT exist in default namespace
    assert!(
        !c.image_exists("test-image")
            .await
            .expect("exists in default ns")
    );

    c.image_remove(&namespaced_name)
        .await
        .expect("remove from ns");
    assert!(
        !c.image_exists(&namespaced_name)
            .await
            .expect("exists after remove")
    );
}

// ---------------------------------------------------------------------------
// Device map/unmap (requires root for sysfs writes)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_device_map_unmap() {
    // Skip if not root
    if !nix::unistd::geteuid().is_root() {
        eprintln!("SKIPPED: test_device_map_unmap requires root");
        return;
    }

    let c = client();
    let name = test_name("devmap");

    c.image_create(&name, 32).await.expect("create");

    let dev_path = c.device_map(&name).await.expect("device_map");
    assert!(
        dev_path.to_str().unwrap_or("").starts_with("/dev/rbd"),
        "device path should start with /dev/rbd, got: {dev_path:?}"
    );
    assert!(dev_path.exists(), "device should exist at {dev_path:?}");

    c.device_unmap(&dev_path).await.expect("device_unmap");
    // Give kernel a moment to clean up
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    c.image_remove(&name).await.expect("remove");
}

#[tokio::test]
async fn test_device_map_namespaced() {
    if !nix::unistd::geteuid().is_root() {
        eprintln!("SKIPPED: test_device_map_namespaced requires root");
        return;
    }

    let c = client();
    let ns = format!("cephalopod-client-devns-{}", std::process::id());
    c.namespace_ensure(&ns).await.expect("ensure ns");

    let name = format!("{ns}/devmap-nsimg");
    c.image_create(&name, 32).await.expect("create");

    let dev_path = c.device_map(&name).await.expect("device_map");
    assert!(dev_path.exists(), "device should exist at {dev_path:?}");

    c.device_unmap(&dev_path).await.expect("device_unmap");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    c.image_remove(&name).await.expect("remove");
}

#[tokio::test]
async fn test_device_unmap_invalid_path() {
    let c = client();
    let err = c.device_unmap("/dev/not-an-rbd-device").await;
    assert!(err.is_err(), "unmap of non-rbd path should fail");
}

#[tokio::test]
async fn test_cross_namespace_clone() {
    let c = client();
    let ns = format!("cephalopod-client-xns-{}", std::process::id());
    c.namespace_ensure(&ns).await.expect("ensure ns");

    let src = format!("{ns}/base-image");
    let dst = test_name("xns-child"); // default namespace

    c.image_create(&src, 32).await.expect("create src in ns");
    c.snap_create(&src, "s1").await.expect("snap_create");
    c.snap_protect(&src, "s1").await.expect("protect");

    // Clone from namespace to default namespace
    c.snap_clone(&src, "s1", &dst)
        .await
        .expect("cross-ns clone");
    assert!(
        c.image_exists(&dst)
            .await
            .expect("child exists in default ns")
    );

    // Cleanup
    c.image_remove(&dst).await.expect("remove child");
    c.snap_unprotect(&src, "s1").await.expect("unprotect");
    c.snap_remove(&src, "s1").await.expect("snap_remove");
    c.image_remove(&src).await.expect("remove src");
}

// ---------------------------------------------------------------------------
// RbdSnapName convenience methods
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_snap_create_named() {
    let c = client();
    let img = test_name("named-create");
    c.image_create(&img, 32).await.expect("create");

    let snap = RbdSnapName {
        image_name: img.clone(),
        snap_name: "s1".to_string(),
    };
    c.snap_create_named(&snap).await.expect("snap_create_named");

    let snaps = c.snap_list(&img).await.expect("snap_list");
    assert_eq!(snaps.len(), 1);
    assert_eq!(snaps[0].name, "s1");

    c.snap_remove_named(&snap).await.expect("snap_remove_named");
    c.image_remove(&img).await.expect("remove");
}

#[tokio::test]
async fn test_snap_protect_unprotect_named() {
    let c = client();
    let img = test_name("named-prot");
    c.image_create(&img, 32).await.expect("create");

    let snap = RbdSnapName {
        image_name: img.clone(),
        snap_name: "s1".to_string(),
    };
    c.snap_create_named(&snap).await.expect("create");
    c.snap_protect_named(&snap).await.expect("protect");

    // Can't remove while protected
    assert!(c.snap_remove_named(&snap).await.is_err());

    c.snap_unprotect_named(&snap).await.expect("unprotect");
    c.snap_remove_named(&snap).await.expect("remove snap");
    c.image_remove(&img).await.expect("remove");
}

#[tokio::test]
async fn test_snap_clone_named() {
    let c = client();
    let parent = test_name("named-clone-parent");
    let child = test_name("named-clone-child");
    c.image_create(&parent, 32).await.expect("create");

    let snap = RbdSnapName {
        image_name: parent.clone(),
        snap_name: "s1".to_string(),
    };
    c.snap_create_named(&snap).await.expect("create snap");
    c.snap_protect_named(&snap).await.expect("protect");

    c.snap_clone_named(&snap, &child)
        .await
        .expect("clone_named");
    assert!(c.image_exists(&child).await.expect("child exists"));

    assert!(
        c.snap_has_children_named(&snap)
            .await
            .expect("has_children")
    );

    // Cleanup
    c.image_remove(&child).await.expect("remove child");
    c.snap_unprotect_named(&snap).await.expect("unprotect");
    c.snap_remove_named(&snap).await.expect("remove snap");
    c.image_remove(&parent).await.expect("remove parent");
}

#[tokio::test]
async fn test_snap_has_children_named_empty() {
    let c = client();
    let img = test_name("named-no-kids");
    c.image_create(&img, 32).await.expect("create");

    let snap = RbdSnapName {
        image_name: img.clone(),
        snap_name: "s1".to_string(),
    };
    c.snap_create_named(&snap).await.expect("create snap");
    c.snap_protect_named(&snap).await.expect("protect");

    assert!(
        !c.snap_has_children_named(&snap)
            .await
            .expect("has_children")
    );

    c.snap_unprotect_named(&snap).await.expect("unprotect");
    c.snap_remove_named(&snap).await.expect("remove snap");
    c.image_remove(&img).await.expect("remove");
}

#[tokio::test]
async fn test_snap_list_named() {
    let c = client();
    let img = test_name("named-list");
    c.image_create(&img, 32).await.expect("create");

    c.snap_create(&img, "a").await.expect("snap a");
    c.snap_create(&img, "b").await.expect("snap b");

    let snaps = c.snap_list_named(&img).await.expect("snap_list_named");
    assert_eq!(snaps.len(), 2);
    // Should be RbdSnapName structs with correct image_name
    assert!(snaps.iter().all(|s| s.image_name == img));
    let names: Vec<&str> = snaps.iter().map(|s| s.snap_name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));

    c.snap_purge(&img).await.expect("purge");
    c.image_remove(&img).await.expect("remove");
}

// ---------------------------------------------------------------------------
// default_client()
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_default_client() {
    let c = cephalopod::default_client().expect("default_client");
    // Should be able to do basic operations
    let exists = c
        .image_exists("cephalopod-nonexistent")
        .await
        .expect("exists");
    assert!(!exists);
}

#[tokio::test]
async fn test_default_client_is_same_instance() {
    let c1 = cephalopod::default_client().expect("default_client 1");
    let c2 = cephalopod::default_client().expect("default_client 2");
    // Both should be references to the same static Client
    assert!(std::ptr::eq(c1, c2));
}

// ---------------------------------------------------------------------------
// Ceph diagnostic commands
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ceph_status() {
    let c = client();
    let status = c.ceph_status().await.expect("ceph_status");
    assert!(
        status.contains("health:"),
        "status should contain health info, got: {status}"
    );
}

#[tokio::test]
async fn test_ceph_client_version() {
    let c = client();
    let version = c.ceph_client_version().await.expect("ceph_client_version");
    assert!(
        version.contains("ceph"),
        "should contain 'ceph', got: {version}"
    );
}

#[tokio::test]
async fn test_ceph_cluster_version() {
    let c = client();
    let version = c
        .ceph_cluster_version()
        .await
        .expect("ceph_cluster_version");
    assert!(!version.is_empty(), "cluster version should not be empty");
}
