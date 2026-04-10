use std::path::Path;
use tokio::process::Command;

/// Runs `resize2fs` on the given device path, allowing unmounted/ext4 resizing.
/// Returns Ok(()) if it succeeds, or an error if it fails.
///
/// # Arguments
/// * `device_path` - Path to the device (e.g., /dev/xyz)
pub async fn resize2fs<P: AsRef<Path>>(device_path: P) -> std::io::Result<()> {
    let output = Command::new("resize2fs")
        .arg(device_path.as_ref())
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "resize2fs failed with status: {}. Stdout: {}. Stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ),
        ))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

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
    async fn resize2fs_grows_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("grow.img");

        // Create a 4 MiB ext4 image
        create_ext4_image(&img, 4).await;

        // Expand the backing file to 8 MiB
        tokio::process::Command::new("truncate")
            .args(["-s", "8M"])
            .arg(&img)
            .status()
            .await
            .unwrap()
            .success()
            .then_some(())
            .expect("truncate failed");

        // resize2fs should grow the filesystem to fill the file
        resize2fs(&img).await.unwrap();

        // Verify: run fsck to confirm the resized filesystem is valid
        tokio::process::Command::new("fsck")
            .args(["-f", "-y"])
            .arg(&img)
            .status()
            .await
            .unwrap()
            .success()
            .then_some(())
            .expect("fsck failed after resize");
    }

    #[tokio::test]
    async fn resize2fs_noop_on_already_full() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("full.img");

        // Create a 4 MiB ext4 image — filesystem already fills the file
        create_ext4_image(&img, 4).await;

        // resize2fs with no extra space should still succeed (no-op)
        resize2fs(&img).await.unwrap();
    }

    #[tokio::test]
    async fn resize2fs_fails_on_non_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("garbage.img");

        tokio::fs::write(&img, vec![0u8; 4096]).await.unwrap();

        let result = resize2fs(&img).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resize2fs_fails_on_missing_file() {
        let result = resize2fs("/tmp/nonexistent_resize2fs_test_image").await;
        assert!(result.is_err());
    }
}
