use std::path::Path;

use tokio::process::Command;
use tracing::debug;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CopyDeviceNodeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to execute sudo command: {0}")]
    CommandFailed(String),
}

/// Copy a device node to the specified destination, resolving any symlinks first.
/// If the source is a symlink, follows it once to get the actual device node.
pub async fn copy_device_node<P1: AsRef<Path>, P2: AsRef<Path>>(
    source_path: P1,
    dest_path: P2,
) -> Result<(), CopyDeviceNodeError> {
    let source = source_path.as_ref();
    let dest = dest_path.as_ref();

    // Resolve symlink if needed
    let actual_device = if source.is_symlink() {
        let link_target = std::fs::read_link(source)?;
        if link_target.is_absolute() {
            link_target
        } else {
            source.parent().unwrap_or(Path::new("")).join(link_target)
        }
    } else {
        source.to_path_buf()
    };

    // Make sure the destination directory exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Copy the actual device node to the destination
    debug!(
        source = %actual_device.display(),
        dest = %dest.display(),
        "Copying device node"
    );

    let output = Command::new("sudo")
        .arg("cp")
        .arg("-a") // Preserve all attributes including device file type
        .arg(&actual_device)
        .arg(&dest)
        .output()
        .await?;

    if !output.status.success() {
        return Err(CopyDeviceNodeError::CommandFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    debug!(dest = %dest.display(), "Device node copied and ownership set");

    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use nix::unistd::geteuid;

    fn is_root() -> bool {
        geteuid().is_root()
    }

    #[tokio::test]
    async fn copies_dev_null() {
        if !is_root() {
            eprintln!("skipping copies_dev_null (requires root)");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("null_copy");

        copy_device_node("/dev/null", &dest).await.unwrap();
        assert!(dest.exists());

        // The copy should be a character device, same as /dev/null
        let src_meta = std::fs::metadata("/dev/null").unwrap();
        let dst_meta = std::fs::metadata(&dest).unwrap();

        use std::os::linux::fs::MetadataExt;
        assert_eq!(
            src_meta.st_rdev(),
            dst_meta.st_rdev(),
            "device major/minor should match"
        );
    }

    #[tokio::test]
    async fn resolves_symlink_before_copy() {
        if !is_root() {
            eprintln!("skipping resolves_symlink_before_copy (requires root)");
            return;
        }

        let dir = tempfile::tempdir().unwrap();

        // Create a symlink pointing to /dev/null
        let link_path = dir.path().join("null_link");
        std::os::unix::fs::symlink("/dev/null", &link_path).unwrap();

        let dest = dir.path().join("null_copy");
        copy_device_node(&link_path, &dest).await.unwrap();

        // The result should be the actual device node, not a symlink
        let meta = std::fs::symlink_metadata(&dest).unwrap();
        assert!(
            !meta.file_type().is_symlink(),
            "destination should be an actual device node, not a symlink"
        );
    }

    #[tokio::test]
    async fn creates_parent_directories() {
        if !is_root() {
            eprintln!("skipping creates_parent_directories (requires root)");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a/b/c/null_copy");

        copy_device_node("/dev/null", &dest).await.unwrap();
        assert!(dest.exists());
    }

    #[tokio::test]
    async fn fails_on_nonexistent_source() {
        if !is_root() {
            eprintln!("skipping fails_on_nonexistent_source (requires root)");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out");

        let result = copy_device_node("/dev/nonexistent_device_node_test", &dest).await;
        assert!(result.is_err());
    }
}
