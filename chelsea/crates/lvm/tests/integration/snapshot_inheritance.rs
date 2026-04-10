use std::{future::Future, path::Path, pin::Pin};
use tokio::process::Command;

use lvm::core::{LayeredFs, LayeredFsCreateOptions};
use tracing::debug;
use uuid::Uuid;

#[tokio::test]
#[ignore]
async fn snapshot_inheritance() {
    // Create the layered filesystem with default options
    debug!("Creating LayeredFs");
    let root_fs = LayeredFs::new(
        LayeredFsCreateOptions::default(),
        Uuid::new_v4().to_string(),
    )
    .await
    .unwrap();

    // Create snapshot chain
    debug!("Creating snapshots... (0/3)");
    let snap0 = root_fs.root_volume.snapshot(None).await.unwrap();
    debug!("(1/3)");
    let snap1 = snap0.snapshot(None).await.unwrap();
    debug!("(2/3)");
    let snap2 = snap1.snapshot(None).await.unwrap();
    debug!("(3/3)");

    // DEBUG
    let output = String::from_utf8(Command::new("lvs").output().await.unwrap().stdout).unwrap();
    debug!("DEBUG: lvs\n{}", output);

    // Write test file to snap1
    with_mounted_device(&snap1.path().unwrap(), |path| {
        Box::pin(
            Command::new("sh")
                .arg("-c")
                .arg(format!("echo 'test data' > {}/testfile", path.display()))
                .status(),
        )
    })
    .await
    .unwrap();
    debug!("Created file /testfile on snap1");

    // Create another snapshot from snap1
    let snap3 = snap1.snapshot(None).await.unwrap();

    // Now verify each snapshot
    // snap0 (shouldn't have file)
    let snap0_result = with_mounted_device(&snap0.path().unwrap(), |path| {
        Box::pin(Command::new("cat").arg(path.join("testfile")).output())
    })
    .await
    .unwrap();
    assert!(!snap0_result.status.success());
    debug!("snap0 does not have file");

    // snap1 (should have file)
    let snap1_result = with_mounted_device(&snap1.path().unwrap(), |path| {
        Box::pin(Command::new("cat").arg(path.join("testfile")).output())
    })
    .await
    .unwrap();
    assert_eq!(
        String::from_utf8(snap1_result.stdout).unwrap(),
        "test data\n"
    );
    debug!("snap1 has file");

    // snap2 (shouldn't have file)
    let snap2_result = with_mounted_device(&snap2.path().unwrap(), |path| {
        Box::pin(Command::new("cat").arg(path.join("testfile")).output())
    })
    .await
    .unwrap();
    assert!(!snap2_result.status.success());
    debug!("snap2 does not have file");

    // snap3 (should have file)
    let snap3_result = with_mounted_device(&snap3.path().unwrap(), |path| {
        Box::pin(Command::new("cat").arg(path.join("testfile")).output())
    })
    .await
    .unwrap();
    assert_eq!(
        String::from_utf8(snap3_result.stdout).unwrap(),
        "test data\n"
    );
    debug!("snap3 has file");
}

// Runs the provided closure after mounting the device specified in the path, guaranteeing an attempt at unmounting in the event of an error
async fn with_mounted_device<R>(
    device_path: &Path,
    f: fn(&std::path::Path) -> Pin<Box<dyn Future<Output = std::io::Result<R>>>>,
) -> std::io::Result<R> {
    let mount_point = tempfile::tempdir()?;

    // Mount and run operation, ensuring we always try to unmount after
    let result = async {
        Command::new("mount")
            .arg(device_path)
            .arg(mount_point.path())
            .status()
            .await
            .unwrap();

        f(mount_point.path()).await
    }
    .await;

    // Always try to unmount, even if the operation failed
    let _unmount_result = Command::new("umount")
        .arg(mount_point.path())
        .status()
        .await;

    result
}
