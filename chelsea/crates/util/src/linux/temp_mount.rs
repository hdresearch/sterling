use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::task;
use tracing::warn;

use crate::temp_dir::{create_temp_dir, TempDir};

#[derive(Debug, Serialize, Clone)]
pub enum MountError {
    Io(String),
    FromUtf8Error(String),
    Mount(String),
    Unmount(String),
}

impl std::error::Error for MountError {}

impl std::fmt::Display for MountError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            MountError::Io(error) => write!(f, "IO error: {}", error),
            MountError::FromUtf8Error(error) => write!(f, "UTF-8 error: {}", error),
            MountError::Mount(error) => write!(f, "Mount error: {}", error),
            MountError::Unmount(error) => write!(f, "Unmount error: {}", error),
        }
    }
}

/// A temporary mount point; will be unmounted and cleaned on drop
pub struct TempMountPoint {
    pub temp_dir: TempDir,
}

impl TempMountPoint {
    pub fn path(&self) -> &Path {
        &self.temp_dir.path
    }
}

impl TempMountPoint {
    fn new(temp_dir: TempDir) -> Self {
        Self { temp_dir }
    }
}

impl Drop for TempMountPoint {
    fn drop(&mut self) {
        if let Err(error) = umount(&self.temp_dir.path) {
            warn!(%error, "Failed to unmount");
        }
    }
}

async fn mount(device_path: &Path, mount_point: &Path) -> Result<(), MountError> {
    let source = device_path.to_path_buf();
    let target = mount_point.to_path_buf();

    task::spawn_blocking(move || blocking_mount(&source, &target))
        .await
        .map_err(|e| MountError::Io(format!("mount task failed: {}", e)))?
}

pub async fn mount_temp(source: &Path, mount_point: PathBuf) -> Result<TempMountPoint, MountError> {
    let mount_dir = create_temp_dir(mount_point, true)
        .await
        .map_err(|e| MountError::Io(e.to_string()))?;
    mount(source, &mount_dir.path).await?;
    Ok(TempMountPoint::new(mount_dir))
}

fn umount(mount_point: &Path) -> Result<(), MountError> {
    blocking_umount(mount_point)
}

#[cfg(target_os = "linux")]
fn blocking_mount(source: &Path, target: &Path) -> Result<(), MountError> {
    use nix::errno::Errno;
    use nix::mount::{mount as nix_mount, MsFlags};

    let attempts: [Option<&'static str>; 3] = [None, Some("ext4"), Some("xfs")];
    let mut last_error: Option<(Option<&'static str>, Errno)> = None;

    for fstype in attempts {
        match nix_mount(
            Some(source),
            target,
            fstype,
            MsFlags::empty(),
            Option::<&str>::None,
        ) {
            Ok(()) => return Ok(()),
            Err(err) => {
                if err == Errno::EINVAL {
                    last_error = Some((fstype, err));
                    continue;
                }
                return Err(MountError::Mount(format!(
                    "failed to mount {} on {} (fstype {:?}): {}",
                    source.display(),
                    target.display(),
                    fstype,
                    err,
                )));
            }
        }
    }

    let (fstype, err) = last_error.unwrap_or((None, Errno::EINVAL));
    Err(MountError::Mount(format!(
        "failed to mount {} on {} (fstype {:?}): {}",
        source.display(),
        target.display(),
        fstype,
        err
    )))
}

#[cfg(target_os = "linux")]
fn blocking_umount(target: &Path) -> Result<(), MountError> {
    use nix::mount::{umount2, MntFlags};

    umount2(target, MntFlags::empty()).map_err(|err| {
        MountError::Unmount(format!("failed to unmount {}: {}", target.display(), err))
    })
}

#[cfg(not(target_os = "linux"))]
fn blocking_mount(device_path: &Path, mount_point: &Path) -> Result<(), MountError> {
    std::process::Command::new("mount")
        .arg(device_path)
        .arg(mount_point)
        .status()
        .map_err(|e| MountError::Io(e.to_string()))?
        .success()
        .then_some(())
        .ok_or_else(|| MountError::Mount("mount failed with non-zero exit status".into()))
}

#[cfg(not(target_os = "linux"))]
fn blocking_umount(mount_point: &Path) -> Result<(), MountError> {
    std::process::Command::new("umount")
        .arg(mount_point)
        .status()
        .map_err(|e| MountError::Io(e.to_string()))?
        .success()
        .then_some(())
        .ok_or_else(|| MountError::Unmount("umount failed with non-zero exit status".into()))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use nix::unistd::geteuid;

    fn is_root() -> bool {
        geteuid().is_root()
    }

    /// Helper: create a small ext4 filesystem image at the given path.
    async fn create_ext4_image(path: &Path, size_mib: u32) {
        tokio::process::Command::new("dd")
            .args([
                "if=/dev/zero",
                &format!("of={}", path.display()),
                "bs=1M",
                &format!("count={size_mib}"),
                "status=none",
            ])
            .status()
            .await
            .unwrap()
            .success()
            .then_some(())
            .expect("dd failed");

        tokio::process::Command::new("mkfs.ext4")
            .args(["-F", "-q"])
            .arg(path)
            .status()
            .await
            .unwrap()
            .success()
            .then_some(())
            .expect("mkfs.ext4 failed");
    }

    #[tokio::test]
    async fn mount_and_unmount_ext4_image() {
        if !is_root() {
            eprintln!("skipping mount_and_unmount_ext4_image (requires root)");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("test.img");
        create_ext4_image(&img, 4).await;

        let mount_point = dir.path().join("mnt");
        let tmp_mount = mount_temp(&img, mount_point.clone()).await.unwrap();

        // The mount point should exist and be accessible
        assert!(tmp_mount.path().is_dir());

        // ext4 creates a lost+found directory
        assert!(tmp_mount.path().join("lost+found").is_dir());

        // Drop should unmount
        drop(tmp_mount);

        // After unmount the directory is cleaned up (recursive_delete=true)
        assert!(!mount_point.exists());
    }

    #[tokio::test]
    async fn can_read_write_on_mount() {
        if !is_root() {
            eprintln!("skipping can_read_write_on_mount (requires root)");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("rw.img");
        create_ext4_image(&img, 4).await;

        let mount_point = dir.path().join("mnt");
        let tmp_mount = mount_temp(&img, mount_point).await.unwrap();

        // Write a file to the mounted filesystem
        let test_file = tmp_mount.path().join("hello.txt");
        tokio::fs::write(&test_file, b"hello from mount")
            .await
            .unwrap();

        // Read it back
        let contents = tokio::fs::read(&test_file).await.unwrap();
        assert_eq!(contents, b"hello from mount");
    }

    #[tokio::test]
    async fn mount_fails_on_non_filesystem() {
        if !is_root() {
            eprintln!("skipping mount_fails_on_non_filesystem (requires root)");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("garbage.img");
        tokio::fs::write(&img, vec![0u8; 4096]).await.unwrap();

        let mount_point = dir.path().join("mnt");
        let result = mount_temp(&img, mount_point).await;
        assert!(result.is_err());
    }
}
