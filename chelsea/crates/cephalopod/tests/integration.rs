//! Integration tests for cephalopod — requires a running Ceph cluster
//! with the "chelsea" client keyring and an "rbd" pool.
//!
//! Run with: cargo nextest run -p cephalopod

use cephalopod::rbd;
use cephalopod::{CephalopodError, RadosCluster, RadosIoCtx};

/// Helper: connect and get an ioctx for the "rbd" pool.
fn connect() -> (RadosCluster, RadosIoCtx) {
    let cluster = RadosCluster::connect("chelsea").expect("connect to ceph cluster");
    let ioctx = cluster.ioctx("rbd").expect("open rbd pool");
    (cluster, ioctx)
}

/// Generate a unique image name for testing.
fn test_image_name(suffix: &str) -> String {
    format!("cephalopod-test-{}-{suffix}", std::process::id())
}

// ---------------------------------------------------------------------------
// Connection tests
// ---------------------------------------------------------------------------

#[test]
fn test_connect() {
    let (_cluster, _ioctx) = connect();
}

#[test]
fn test_connect_bad_user() {
    let result = RadosCluster::connect("nonexistent_user_xyz");
    assert!(result.is_err(), "should fail with bad user");
}

#[test]
fn test_ioctx_bad_pool() {
    let cluster = RadosCluster::connect("chelsea").expect("connect");
    let result = cluster.ioctx("nonexistent_pool_xyz");
    assert!(result.is_err(), "should fail with bad pool");
}

// ---------------------------------------------------------------------------
// Image CRUD tests
// ---------------------------------------------------------------------------

#[test]
fn test_image_create_stat_remove() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("crud");

    // Create
    rbd::image_create(&ioctx, &name, 64).expect("create image");

    // Exists
    assert!(rbd::image_exists(&ioctx, &name).expect("image_exists"));

    // Stat
    let info = rbd::image_stat(&ioctx, &name).expect("stat image");
    assert_eq!(info.size_mib(), 64);
    assert_eq!(info.size, 64 * 1024 * 1024);

    // Get size
    let size = rbd::image_get_size(&ioctx, &name).expect("get_size");
    assert_eq!(size, 64 * 1024 * 1024);

    // List — should contain our image
    let images = rbd::image_list(&ioctx).expect("image_list");
    assert!(images.contains(&name), "image_list should contain {name}");

    // Remove
    rbd::image_remove(&ioctx, &name).expect("remove image");

    // Should be gone
    assert!(!rbd::image_exists(&ioctx, &name).expect("image_exists after remove"));
}

#[test]
fn test_image_exists_nonexistent() {
    let (_cluster, ioctx) = connect();
    assert!(!rbd::image_exists(&ioctx, "cephalopod-does-not-exist-xyz").expect("image_exists"));
}

#[test]
fn test_image_resize() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("resize");

    rbd::image_create(&ioctx, &name, 64).expect("create");

    // Grow to 128 MiB
    rbd::image_resize(&ioctx, &name, 128).expect("resize");
    let info = rbd::image_stat(&ioctx, &name).expect("stat");
    assert_eq!(info.size_mib(), 128);

    // Cleanup
    rbd::image_remove(&ioctx, &name).expect("remove");
}

// ---------------------------------------------------------------------------
// Snapshot tests
// ---------------------------------------------------------------------------

#[test]
fn test_snap_create_list_remove() {
    let (_cluster, ioctx) = connect();
    let img = test_image_name("snap");

    rbd::image_create(&ioctx, &img, 32).expect("create");

    // Create snap
    rbd::snap_create(&ioctx, &img, "snap1").expect("snap_create");

    // List snaps
    let snaps = rbd::snap_list(&ioctx, &img).expect("snap_list");
    assert_eq!(snaps.len(), 1);
    assert_eq!(snaps[0].name, "snap1");

    // Remove snap
    rbd::snap_remove(&ioctx, &img, "snap1").expect("snap_remove");
    let snaps = rbd::snap_list(&ioctx, &img).expect("snap_list after remove");
    assert_eq!(snaps.len(), 0);

    // Cleanup
    rbd::image_remove(&ioctx, &img).expect("remove");
}

#[test]
fn test_snap_protect_unprotect() {
    let (_cluster, ioctx) = connect();
    let img = test_image_name("protect");

    rbd::image_create(&ioctx, &img, 32).expect("create");
    rbd::snap_create(&ioctx, &img, "snap1").expect("snap_create");

    // Protect
    rbd::snap_protect(&ioctx, &img, "snap1").expect("snap_protect");
    assert!(rbd::snap_is_protected(&ioctx, &img, "snap1").expect("is_protected"));

    // Can't remove while protected
    let err = rbd::snap_remove(&ioctx, &img, "snap1");
    assert!(err.is_err(), "should fail to remove protected snap");

    // Unprotect
    rbd::snap_unprotect(&ioctx, &img, "snap1").expect("snap_unprotect");
    assert!(!rbd::snap_is_protected(&ioctx, &img, "snap1").expect("is_protected after unprotect"));

    // Now can remove
    rbd::snap_remove(&ioctx, &img, "snap1").expect("snap_remove");
    rbd::image_remove(&ioctx, &img).expect("remove");
}

#[test]
fn test_snap_purge() {
    let (_cluster, ioctx) = connect();
    let img = test_image_name("purge");

    rbd::image_create(&ioctx, &img, 32).expect("create");
    rbd::snap_create(&ioctx, &img, "s1").expect("snap_create s1");
    rbd::snap_create(&ioctx, &img, "s2").expect("snap_create s2");

    let snaps = rbd::snap_list(&ioctx, &img).expect("snap_list");
    assert_eq!(snaps.len(), 2);

    rbd::snap_purge(&ioctx, &img).expect("snap_purge");

    let snaps = rbd::snap_list(&ioctx, &img).expect("snap_list after purge");
    assert_eq!(snaps.len(), 0);

    rbd::image_remove(&ioctx, &img).expect("remove");
}

// ---------------------------------------------------------------------------
// Clone and children tests
// ---------------------------------------------------------------------------

#[test]
fn test_snap_clone_and_children() {
    let (_cluster, ioctx) = connect();
    let parent = test_image_name("clone-parent");
    let child = test_image_name("clone-child");

    rbd::image_create(&ioctx, &parent, 32).expect("create parent");
    rbd::snap_create(&ioctx, &parent, "snap1").expect("snap_create");
    rbd::snap_protect(&ioctx, &parent, "snap1").expect("snap_protect");

    // Clone
    rbd::snap_clone(&ioctx, &parent, "snap1", &ioctx, &child).expect("snap_clone");

    // Child should exist
    assert!(rbd::image_exists(&ioctx, &child).expect("child exists"));

    // Children list
    let children = rbd::snap_list_children(&ioctx, &parent, "snap1").expect("list_children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].image_name, child);

    // Cleanup: remove child first, then unprotect+remove parent snap, then remove parent
    rbd::image_remove(&ioctx, &child).expect("remove child");
    rbd::snap_unprotect(&ioctx, &parent, "snap1").expect("unprotect");
    rbd::snap_remove(&ioctx, &parent, "snap1").expect("snap_remove");
    rbd::image_remove(&ioctx, &parent).expect("remove parent");
}

// ---------------------------------------------------------------------------
// Watcher tests
// ---------------------------------------------------------------------------

#[test]
fn test_image_watchers() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("watchers");

    rbd::image_create(&ioctx, &name, 32).expect("create");

    // image_watchers opens the image internally which may register a watcher.
    // Just verify the call succeeds and returns a valid list.
    let watchers = rbd::image_watchers(&ioctx, &name).expect("watchers");
    // The open-for-stat may or may not count as a watcher depending on librbd version,
    // so we just check it doesn't error.
    let _ = watchers;

    rbd::image_remove(&ioctx, &name).expect("remove");
}

// ---------------------------------------------------------------------------
// Namespace tests
// ---------------------------------------------------------------------------

#[test]
fn test_namespace_create_exists_list() {
    let (_cluster, ioctx) = connect();
    let ns = format!("cephalopod-test-ns-{}", std::process::id());

    // Should not exist yet
    assert!(!rbd::namespace_exists(&ioctx, &ns).expect("namespace_exists"));

    // Create
    rbd::namespace_create(&ioctx, &ns).expect("namespace_create");

    // Should exist now
    assert!(rbd::namespace_exists(&ioctx, &ns).expect("namespace_exists after create"));

    // Should appear in list
    let namespaces = rbd::namespace_list(&ioctx).expect("namespace_list");
    assert!(
        namespaces.contains(&ns),
        "namespace_list should contain {ns}"
    );

    // Creating again should fail with AlreadyExists
    let err = rbd::namespace_create(&ioctx, &ns);
    assert!(
        matches!(err, Err(CephalopodError::AlreadyExists(_))),
        "double create should be AlreadyExists, got: {err:?}"
    );

    // Cleanup: namespace_remove isn't exposed yet, but the namespace is empty so it's harmless.
    // We can't easily remove it without the remove binding, but the test namespace is unique per PID.
}

// ---------------------------------------------------------------------------
// Edge case / error tests
// ---------------------------------------------------------------------------

#[test]
fn test_image_remove_nonexistent() {
    let (_cluster, ioctx) = connect();
    let err = rbd::image_remove(&ioctx, "cephalopod-does-not-exist-xyz");
    assert!(
        matches!(err, Err(CephalopodError::NotFound(_))),
        "removing nonexistent image should be NotFound, got: {err:?}"
    );
}

#[test]
fn test_image_stat_nonexistent() {
    let (_cluster, ioctx) = connect();
    let err = rbd::image_stat(&ioctx, "cephalopod-does-not-exist-xyz");
    assert!(
        matches!(err, Err(CephalopodError::NotFound(_))),
        "stat on nonexistent image should be NotFound, got: {err:?}"
    );
}

#[test]
fn test_image_remove_with_snaps_fails() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("rm-with-snaps");

    rbd::image_create(&ioctx, &name, 32).expect("create");
    rbd::snap_create(&ioctx, &name, "s1").expect("snap_create");

    // Should fail — image has snapshots
    let err = rbd::image_remove(&ioctx, &name);
    assert!(err.is_err(), "remove with snaps should fail, got: {err:?}");

    // Cleanup
    rbd::snap_remove(&ioctx, &name, "s1").expect("snap_remove");
    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_image_create_duplicate() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("duplicate");

    rbd::image_create(&ioctx, &name, 32).expect("create");

    let err = rbd::image_create(&ioctx, &name, 32);
    assert!(
        matches!(err, Err(CephalopodError::AlreadyExists(_))),
        "duplicate create should be AlreadyExists, got: {err:?}"
    );

    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_snap_create_on_nonexistent_image() {
    let (_cluster, ioctx) = connect();
    let err = rbd::snap_create(&ioctx, "cephalopod-does-not-exist-xyz", "snap1");
    assert!(
        matches!(err, Err(CephalopodError::NotFound(_))),
        "snap_create on nonexistent image should be NotFound, got: {err:?}"
    );
}

#[test]
fn test_snap_remove_nonexistent_snap() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("rm-nosnap");

    rbd::image_create(&ioctx, &name, 32).expect("create");

    let err = rbd::snap_remove(&ioctx, &name, "nonexistent-snap");
    assert!(
        matches!(err, Err(CephalopodError::NotFound(_))),
        "removing nonexistent snap should be NotFound, got: {err:?}"
    );

    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_snap_protect_already_protected() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("double-protect");

    rbd::image_create(&ioctx, &name, 32).expect("create");
    rbd::snap_create(&ioctx, &name, "s1").expect("snap_create");
    rbd::snap_protect(&ioctx, &name, "s1").expect("protect");

    // Protecting again should fail
    let err = rbd::snap_protect(&ioctx, &name, "s1");
    assert!(err.is_err(), "double protect should fail, got: {err:?}");

    // Cleanup
    rbd::snap_unprotect(&ioctx, &name, "s1").expect("unprotect");
    rbd::snap_remove(&ioctx, &name, "s1").expect("snap_remove");
    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_snap_unprotect_not_protected() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("unprotect-nop");

    rbd::image_create(&ioctx, &name, 32).expect("create");
    rbd::snap_create(&ioctx, &name, "s1").expect("snap_create");

    // Unprotecting a snap that isn't protected should fail
    let err = rbd::snap_unprotect(&ioctx, &name, "s1");
    assert!(
        err.is_err(),
        "unprotect on unprotected snap should fail, got: {err:?}"
    );

    // Cleanup
    rbd::snap_remove(&ioctx, &name, "s1").expect("snap_remove");
    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_snap_clone_unprotected_succeeds_with_v2() {
    let (_cluster, ioctx) = connect();
    let parent = test_image_name("clone-unprot-parent");
    let child = test_image_name("clone-unprot-child");

    rbd::image_create(&ioctx, &parent, 32).expect("create");
    rbd::snap_create(&ioctx, &parent, "s1").expect("snap_create");

    // Clone format v2 allows cloning from unprotected snapshots
    rbd::snap_clone(&ioctx, &parent, "s1", &ioctx, &child)
        .expect("clone from unprotected snap (v2)");
    assert!(rbd::image_exists(&ioctx, &child).expect("child exists"));

    // Cleanup
    rbd::image_remove(&ioctx, &child).expect("remove child");
    rbd::snap_remove(&ioctx, &parent, "s1").expect("snap_remove");
    rbd::image_remove(&ioctx, &parent).expect("remove");
}

#[test]
fn test_snap_unprotect_with_v2_children_succeeds() {
    let (_cluster, ioctx) = connect();
    let parent = test_image_name("unprot-children-parent");
    let child = test_image_name("unprot-children-child");

    rbd::image_create(&ioctx, &parent, 32).expect("create");
    rbd::snap_create(&ioctx, &parent, "s1").expect("snap_create");
    rbd::snap_protect(&ioctx, &parent, "s1").expect("protect");
    rbd::snap_clone(&ioctx, &parent, "s1", &ioctx, &child).expect("clone");

    // Clone format v2 allows unprotecting snaps that still have children
    rbd::snap_unprotect(&ioctx, &parent, "s1").expect("unprotect with v2 children");

    // Children still exist and work fine
    let children = rbd::snap_list_children(&ioctx, &parent, "s1").expect("list_children");
    assert_eq!(children.len(), 1);

    // Cleanup
    rbd::image_remove(&ioctx, &child).expect("remove child");
    rbd::snap_remove(&ioctx, &parent, "s1").expect("snap_remove");
    rbd::image_remove(&ioctx, &parent).expect("remove parent");
}

#[test]
fn test_snap_list_children_no_children() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("no-children");

    rbd::image_create(&ioctx, &name, 32).expect("create");
    rbd::snap_create(&ioctx, &name, "s1").expect("snap_create");
    rbd::snap_protect(&ioctx, &name, "s1").expect("protect");

    let children = rbd::snap_list_children(&ioctx, &name, "s1").expect("list_children");
    assert!(children.is_empty(), "should have no children");

    // Cleanup
    rbd::snap_unprotect(&ioctx, &name, "s1").expect("unprotect");
    rbd::snap_remove(&ioctx, &name, "s1").expect("snap_remove");
    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_snap_list_on_image_with_no_snaps() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("no-snaps");

    rbd::image_create(&ioctx, &name, 32).expect("create");
    let snaps = rbd::snap_list(&ioctx, &name).expect("snap_list");
    assert!(snaps.is_empty());
    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_namespace_exists_nonexistent() {
    let (_cluster, ioctx) = connect();
    assert!(
        !rbd::namespace_exists(&ioctx, "cephalopod-ns-does-not-exist-xyz")
            .expect("namespace_exists")
    );
}

#[test]
fn test_image_list_empty_namespace() {
    let (_cluster, ioctx) = connect();
    let ns = format!("cephalopod-test-empty-ns-{}", std::process::id());

    rbd::namespace_create(&ioctx, &ns).expect("namespace_create");
    ioctx.set_namespace(&ns).expect("set_namespace");

    let images = rbd::image_list(&ioctx).expect("image_list");
    assert!(images.is_empty(), "new namespace should have no images");
}

#[test]
fn test_image_resize_same_size_is_noop() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("resize-noop");

    rbd::image_create(&ioctx, &name, 64).expect("create");

    // Resize to same size should succeed
    rbd::image_resize(&ioctx, &name, 64).expect("resize to same size");
    let info = rbd::image_stat(&ioctx, &name).expect("stat");
    assert_eq!(info.size_mib(), 64);

    rbd::image_remove(&ioctx, &name).expect("remove");
}

#[test]
fn test_multiple_snaps_on_same_image() {
    let (_cluster, ioctx) = connect();
    let name = test_image_name("multi-snap");

    rbd::image_create(&ioctx, &name, 32).expect("create");
    rbd::snap_create(&ioctx, &name, "a").expect("snap a");
    rbd::snap_create(&ioctx, &name, "b").expect("snap b");
    rbd::snap_create(&ioctx, &name, "c").expect("snap c");

    let snaps = rbd::snap_list(&ioctx, &name).expect("snap_list");
    assert_eq!(snaps.len(), 3);
    let names: Vec<&str> = snaps.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
    assert!(names.contains(&"c"));

    // Cleanup
    rbd::snap_purge(&ioctx, &name).expect("purge");
    rbd::image_remove(&ioctx, &name).expect("remove");
}

// ---------------------------------------------------------------------------
// Namespace-scoped image operations
// ---------------------------------------------------------------------------

#[test]
fn test_namespace_scoped_images() {
    let (_cluster, ioctx) = connect();
    let ns = format!("cephalopod-test-nsimg-{}", std::process::id());

    // Create namespace
    rbd::namespace_create(&ioctx, &ns).expect("namespace_create");

    // Set namespace on ioctx
    ioctx.set_namespace(&ns).expect("set_namespace");

    let name = "test-image-in-ns";

    // Create image in namespace
    rbd::image_create(&ioctx, name, 32).expect("create in namespace");
    assert!(rbd::image_exists(&ioctx, name).expect("exists in namespace"));

    // Image should appear in list within this namespace
    let images = rbd::image_list(&ioctx).expect("image_list in namespace");
    assert!(images.contains(&name.to_string()));

    // Cleanup
    rbd::image_remove(&ioctx, name).expect("remove from namespace");

    // Switch back to default namespace — image should not be there
    ioctx.set_namespace("").expect("reset namespace");
    assert!(!rbd::image_exists(&ioctx, name).expect("exists in default ns"));
}
