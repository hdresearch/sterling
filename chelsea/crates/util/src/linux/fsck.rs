use std::path::Path;
use tokio::process::Command;

/// Runs `fsck -f -y` on the given device path.
/// Returns `Ok(())` if fsck succeeds (exit status 0), or the appropriate error.
pub async fn fsck_force_yes<P: AsRef<Path>>(device_path: P) -> std::io::Result<()> {
    let output = Command::new("fsck")
        .arg("-f")
        .arg("-y")
        .arg(device_path.as_ref())
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "fsck -f -y failed with status: {}. Stdout: {}. Stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string()
            ),
        ))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Helper: create a small ext4 filesystem image at the given path.
    async fn create_ext4_image(path: &Path, size_mib: u32) {
        // Create a zeroed file of the given size
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

        // Format it as ext4
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
    async fn fsck_succeeds_on_clean_ext4_image() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("clean.img");

        create_ext4_image(&img, 4).await;

        fsck_force_yes(&img).await.unwrap();
    }

    #[tokio::test]
    async fn fsck_fails_on_non_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("garbage.img");

        // Write random garbage — not a valid filesystem
        tokio::fs::write(&img, vec![0xFFu8; 4096]).await.unwrap();

        let result = fsck_force_yes(&img).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fsck_fails_on_missing_file() {
        let result = fsck_force_yes("/tmp/nonexistent_fsck_test_image").await;
        assert!(result.is_err());
    }
}
