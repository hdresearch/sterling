//! Criterion benchmarks for cephalopod RBD operations.
//!
//! Requires a running Ceph cluster with "chelsea" client and "rbd" pool.
//! Device map/unmap benchmarks require root.
//!
//! Run: cargo bench -p cephalopod
//!
//! These benchmarks measure real Ceph operations against a live cluster,
//! so results will vary with cluster load and network conditions.

use criterion::{Criterion, criterion_group, criterion_main};
use std::sync::OnceLock;
use tokio::runtime::Runtime;

use cephalopod::Client;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| Client::connect("chelsea", "rbd").expect("connect"))
}

fn bench_image_name(suffix: &str) -> String {
    format!("cephalopod-bench-{suffix}")
}

/// Ensure a bench image exists. Call at the start of benchmarks that need it.
fn ensure_image(name: &str, size_mib: u32) {
    rt().block_on(async {
        let c = client();
        if !c.image_exists(name).await.unwrap() {
            c.image_create(name, size_mib).await.unwrap();
        }
    });
}

/// Ensure a bench image with a protected snap exists.
fn ensure_image_with_snap(image: &str, snap: &str, size_mib: u32) {
    rt().block_on(async {
        let c = client();
        if !c.image_exists(image).await.unwrap() {
            c.image_create(image, size_mib).await.unwrap();
        }
        let snaps = c.snap_list(image).await.unwrap();
        if !snaps.iter().any(|s| s.name == snap) {
            c.snap_create(image, snap).await.unwrap();
            c.snap_protect(image, snap).await.unwrap();
        }
    });
}

/// Clean up: remove an image if it exists (purge snaps first).
fn cleanup_image(name: &str) {
    rt().block_on(async {
        let c = client();
        if c.image_exists(name).await.unwrap_or(false) {
            let _ = c.snap_purge(name).await;
            let _ = c.image_remove(name).await;
        }
    });
}

// ---------------------------------------------------------------------------
// Connection benchmark
// ---------------------------------------------------------------------------

fn bench_connect(c: &mut Criterion) {
    c.bench_function("connect", |b| {
        b.iter(|| {
            Client::connect("chelsea", "rbd").expect("connect");
        });
    });
}

// ---------------------------------------------------------------------------
// Image operations
// ---------------------------------------------------------------------------

fn bench_image_stat(c: &mut Criterion) {
    let name = bench_image_name("stat");
    ensure_image(&name, 32);

    c.bench_function("image_info", |b| {
        b.to_async(rt()).iter(|| async {
            client().image_info(&name).await.unwrap();
        });
    });

    cleanup_image(&name);
}

fn bench_image_info_repeated(c: &mut Criterion) {
    let name = bench_image_name("stat-repeat");
    ensure_image(&name, 32);

    // Warm the cache with one call
    rt().block_on(async { client().image_info(&name).await.unwrap() });

    c.bench_function("image_info (cached, repeated)", |b| {
        b.to_async(rt()).iter(|| async {
            client().image_info(&name).await.unwrap();
        });
    });

    cleanup_image(&name);
}

fn bench_image_exists(c: &mut Criterion) {
    let name = bench_image_name("exists");
    ensure_image(&name, 32);

    c.bench_function("image_exists (true)", |b| {
        b.to_async(rt()).iter(|| async {
            client().image_exists(&name).await.unwrap();
        });
    });

    c.bench_function("image_exists (false)", |b| {
        b.to_async(rt()).iter(|| async {
            client()
                .image_exists("cephalopod-bench-nonexistent")
                .await
                .unwrap();
        });
    });

    cleanup_image(&name);
}

fn bench_image_create_remove(c: &mut Criterion) {
    let mut counter = 0u64;

    c.bench_function("image_create + image_remove", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let name = format!("cephalopod-bench-cr-{counter}");
            async move {
                let c = client();
                c.image_create(&name, 32).await.unwrap();
                c.image_remove(&name).await.unwrap();
            }
        });
    });
}

fn bench_image_list(c: &mut Criterion) {
    c.bench_function("image_list", |b| {
        b.to_async(rt()).iter(|| async {
            client().image_list().await.unwrap();
        });
    });
}

fn bench_image_has_watchers(c: &mut Criterion) {
    let name = bench_image_name("watchers");
    ensure_image(&name, 32);

    c.bench_function("image_has_watchers", |b| {
        b.to_async(rt()).iter(|| async {
            client().image_has_watchers(&name).await.unwrap();
        });
    });

    cleanup_image(&name);
}

// ---------------------------------------------------------------------------
// Snapshot operations
// ---------------------------------------------------------------------------

fn bench_snap_create_remove(c: &mut Criterion) {
    let img = bench_image_name("snap-cr");
    ensure_image(&img, 32);
    let mut counter = 0u64;

    c.bench_function("snap_create + snap_remove", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let snap = format!("s{counter}");
            let img = img.clone();
            async move {
                let c = client();
                c.snap_create(&img, &snap).await.unwrap();
                c.snap_remove(&img, &snap).await.unwrap();
            }
        });
    });

    cleanup_image(&img);
}

fn bench_snap_create_protect(c: &mut Criterion) {
    let img = bench_image_name("snap-cp");
    ensure_image(&img, 32);
    let mut counter = 0u64;

    c.bench_function("snap_create + snap_protect (commit path)", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let snap = format!("s{counter}");
            let img = img.clone();
            async move {
                let c = client();
                c.snap_create(&img, &snap).await.unwrap();
                c.snap_protect(&img, &snap).await.unwrap();
            }
        });
    });

    // Cleanup: purge all the snaps we created
    cleanup_image(&img);
}

fn bench_snap_create_protect_combined(c: &mut Criterion) {
    let img = bench_image_name("snap-cp-combined");
    ensure_image(&img, 32);
    let mut counter = 0u64;

    c.bench_function("snap_create_and_protect (combined)", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let snap = format!("s{counter}");
            let img = img.clone();
            async move {
                client().snap_create_and_protect(&img, &snap).await.unwrap();
            }
        });
    });

    cleanup_image(&img);
}

fn bench_snap_list(c: &mut Criterion) {
    let img = bench_image_name("snap-list");
    ensure_image(&img, 32);

    // Create a few snaps to list
    rt().block_on(async {
        let c = client();
        for i in 0..5 {
            c.snap_create(&img, &format!("s{i}")).await.unwrap();
        }
    });

    c.bench_function("snap_list (5 snaps)", |b| {
        b.to_async(rt()).iter(|| async {
            client().snap_list(&img).await.unwrap();
        });
    });

    cleanup_image(&img);
}

fn bench_snap_clone(c: &mut Criterion) {
    let parent = bench_image_name("snap-clone-parent");
    ensure_image_with_snap(&parent, "base", 32);
    let mut counter = 0u64;

    c.bench_function("snap_clone (VM boot path)", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let child = format!("cephalopod-bench-clone-{counter}");
            let parent = parent.clone();
            async move {
                let c = client();
                c.snap_clone(&parent, "base", &child).await.unwrap();
                // Cleanup inline to avoid accumulating images
                c.image_remove(&child).await.unwrap();
            }
        });
    });

    cleanup_image(&parent);
}

fn bench_snap_has_children(c: &mut Criterion) {
    let parent = bench_image_name("snap-children");
    ensure_image_with_snap(&parent, "base", 32);

    c.bench_function("snap_has_children (no children)", |b| {
        b.to_async(rt()).iter(|| async {
            client().snap_has_children(&parent, "base").await.unwrap();
        });
    });

    cleanup_image(&parent);
}

// ---------------------------------------------------------------------------
// Device map/unmap (requires root)
// ---------------------------------------------------------------------------

fn bench_device_map_unmap(c: &mut Criterion) {
    if !nix::unistd::geteuid().is_root() {
        eprintln!("SKIPPED: device_map/unmap benchmarks require root");
        return;
    }

    let name = bench_image_name("devmap");
    ensure_image(&name, 32);

    c.bench_function("device_map + device_unmap", |b| {
        b.to_async(rt()).iter(|| async {
            let c = client();
            let dev = c.device_map(&name).await.unwrap();
            c.device_unmap(&dev).await.unwrap();
        });
    });

    cleanup_image(&name);
}

// ---------------------------------------------------------------------------
// Namespace operations
// ---------------------------------------------------------------------------

fn bench_namespace_exists(c: &mut Criterion) {
    let ns = "cephalopod-bench-ns";
    rt().block_on(async {
        let _ = client().namespace_create(ns).await;
    });

    c.bench_function("namespace_exists", |b| {
        b.to_async(rt()).iter(|| async {
            client().namespace_exists(ns).await.unwrap();
        });
    });
}

// ---------------------------------------------------------------------------
// Compound operations (simulating real workflows)
// ---------------------------------------------------------------------------

fn bench_commit_workflow(c: &mut Criterion) {
    let img = bench_image_name("commit-wf");
    ensure_image(&img, 32);
    let mut counter = 0u64;

    c.bench_function("commit workflow (snap_create + protect)", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let snap = format!("commit-{counter}");
            let img = img.clone();
            async move {
                let c = client();
                c.snap_create(&img, &snap).await.unwrap();
                c.snap_protect(&img, &snap).await.unwrap();
            }
        });
    });

    cleanup_image(&img);
}

fn bench_vm_boot_workflow(c: &mut Criterion) {
    if !nix::unistd::geteuid().is_root() {
        eprintln!("SKIPPED: VM boot workflow benchmark requires root");
        return;
    }

    let parent = bench_image_name("boot-wf-parent");
    ensure_image_with_snap(&parent, "base", 32);
    let mut counter = 0u64;

    c.bench_function("VM boot workflow (clone + map)", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let child = format!("cephalopod-bench-boot-{counter}");
            let parent = parent.clone();
            async move {
                let c = client();
                c.snap_clone(&parent, "base", &child).await.unwrap();
                let dev = c.device_map(&child).await.unwrap();
                // Cleanup
                c.device_unmap(&dev).await.unwrap();
                c.image_remove(&child).await.unwrap();
            }
        });
    });

    cleanup_image(&parent);
}

fn bench_vm_shutdown_workflow(c: &mut Criterion) {
    if !nix::unistd::geteuid().is_root() {
        eprintln!("SKIPPED: VM shutdown workflow benchmark requires root");
        return;
    }

    let parent = bench_image_name("shutdown-wf-parent");
    ensure_image_with_snap(&parent, "base", 32);
    let mut counter = 0u64;

    c.bench_function("VM shutdown workflow (unmap + snap + protect)", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let child = format!("cephalopod-bench-shutdown-{counter}");
            let snap = format!("shutdown-snap-{counter}");
            let parent = parent.clone();
            async move {
                let c = client();
                // Setup: clone + map
                c.snap_clone(&parent, "base", &child).await.unwrap();
                let dev = c.device_map(&child).await.unwrap();

                // The actual shutdown path we're measuring:
                c.device_unmap(&dev).await.unwrap();
                c.snap_create(&child, &snap).await.unwrap();
                c.snap_protect(&child, &snap).await.unwrap();

                // Cleanup
                c.snap_unprotect(&child, &snap).await.unwrap();
                c.snap_remove(&child, &snap).await.unwrap();
                c.image_remove(&child).await.unwrap();
            }
        });
    });

    cleanup_image(&parent);
}

// ---------------------------------------------------------------------------
// Exec comparison — same ops via rbd CLI for A/B comparison
// ---------------------------------------------------------------------------

fn bench_exec_image_info(c: &mut Criterion) {
    let name = bench_image_name("exec-stat");
    ensure_image(&name, 32);

    c.bench_function("exec: rbd info --format json", |b| {
        b.to_async(rt()).iter(|| {
            let name = name.clone();
            async move {
                let output = tokio::process::Command::new("rbd")
                    .args([
                        "--id",
                        "chelsea",
                        "info",
                        &format!("rbd/{name}"),
                        "--format",
                        "json",
                    ])
                    .output()
                    .await
                    .unwrap();
                assert!(output.status.success());
            }
        });
    });

    cleanup_image(&name);
}

fn bench_exec_snap_create_remove(c: &mut Criterion) {
    let img = bench_image_name("exec-snap");
    ensure_image(&img, 32);
    let mut counter = 0u64;

    c.bench_function("exec: rbd snap create + rm", |b| {
        b.to_async(rt()).iter(|| {
            counter += 1;
            let snap_spec = format!("rbd/{img}@s{counter}");
            async move {
                let out = tokio::process::Command::new("rbd")
                    .args(["--id", "chelsea", "snap", "create", &snap_spec])
                    .output()
                    .await
                    .unwrap();
                assert!(out.status.success());

                let out = tokio::process::Command::new("rbd")
                    .args(["--id", "chelsea", "snap", "rm", &snap_spec])
                    .output()
                    .await
                    .unwrap();
                assert!(out.status.success());
            }
        });
    });

    cleanup_image(&img);
}

fn bench_exec_image_exists(c: &mut Criterion) {
    let name = bench_image_name("exec-exists");
    ensure_image(&name, 32);

    c.bench_function("exec: rbd info (exists check)", |b| {
        b.to_async(rt()).iter(|| {
            let name = name.clone();
            async move {
                let _ = tokio::process::Command::new("rbd")
                    .args(["--id", "chelsea", "info", &format!("rbd/{name}")])
                    .output()
                    .await
                    .unwrap();
            }
        });
    });

    cleanup_image(&name);
}

// ---------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------

criterion_group!(
    name = connection;
    config = Criterion::default().sample_size(20);
    targets = bench_connect
);

criterion_group!(
    name = image_ops;
    config = Criterion::default().sample_size(20);
    targets =
        bench_image_stat,
        bench_image_info_repeated,
        bench_image_exists,
        bench_image_create_remove,
        bench_image_list,
        bench_image_has_watchers
);

criterion_group!(
    name = snap_ops;
    config = Criterion::default().sample_size(20);
    targets =
        bench_snap_create_remove,
        bench_snap_create_protect,
        bench_snap_create_protect_combined,
        bench_snap_list,
        bench_snap_clone,
        bench_snap_has_children
);

criterion_group!(
    name = device_ops;
    config = Criterion::default().sample_size(10);
    targets = bench_device_map_unmap
);

criterion_group!(
    name = namespace_ops;
    config = Criterion::default().sample_size(20);
    targets = bench_namespace_exists
);

criterion_group!(
    name = workflows;
    config = Criterion::default().sample_size(10);
    targets =
        bench_commit_workflow,
        bench_vm_boot_workflow,
        bench_vm_shutdown_workflow
);

criterion_group!(
    name = exec_comparison;
    config = Criterion::default().sample_size(20);
    targets =
        bench_exec_image_info,
        bench_exec_snap_create_remove,
        bench_exec_image_exists
);

criterion_main!(
    connection,
    image_ops,
    snap_ops,
    device_ops,
    namespace_ops,
    workflows,
    exec_comparison
);
