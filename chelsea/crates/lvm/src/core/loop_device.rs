use tracing::debug;

use super::backing_file::BackingFile;
use crate::error::LvmError;
use std::path::PathBuf;
use tokio::process::Command;

#[derive(Debug)]
pub struct LoopDevice {
    path: PathBuf,
    /// The file to which the loop device points
    pub backing_file: BackingFile,
}

impl LoopDevice {
    pub async fn new(backing_file: BackingFile) -> Result<Self, LvmError> {
        let filepath = backing_file.path_str();

        let output = Command::new("losetup")
            .args(["-f", "--show", &filepath])
            .output()
            .await
            .map_err(|e| LvmError::LoopDeviceCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::LoopDeviceCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());

        debug!(
            ?path,
            backing_file_path = ?filepath,
            "Loop device created",
        );

        Ok(Self { path, backing_file })
    }

    pub fn from_existing(path: PathBuf, backing_file: BackingFile) -> Result<Self, LvmError> {
        match path.exists() {
            true => Ok(Self { path, backing_file }),
            false => Err(LvmError::LoopDeviceFromExisting(path.display().to_string())),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn path_str(&self) -> String {
        self.path().display().to_string()
    }

    pub fn delete(&self) -> Result<(), LvmError> {
        let path = self.path_str();
        debug!(path = %path, "Deleting loop device");

        let output = std::process::Command::new("losetup")
            .args(["-d", &path])
            .output()
            .map_err(|e| {
                debug!(path = %path, error = %e, "Failed to execute losetup command");
                LvmError::LoopDeviceDelete(format!("{}: {}", path, e.to_string()))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            debug!(path = %path, error = %error, "losetup command failed");
            return Err(LvmError::LoopDeviceDelete(format!("{}: {}", path, error)));
        }

        debug!(path = %path, "Successfully deleted loop device");
        self.backing_file.delete()
    }
}
