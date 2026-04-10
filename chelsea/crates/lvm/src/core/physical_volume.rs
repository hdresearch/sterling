use tracing::debug;

use super::loop_device::LoopDevice;
use crate::error::LvmError;
use std::path::PathBuf;
use tokio::process::Command;

#[derive(Debug)]
pub struct PhysicalVolume {
    /// The device on which the volume has been initialized
    pub device: LoopDevice,
}

impl PhysicalVolume {
    pub async fn new(device: LoopDevice) -> Result<Self, LvmError> {
        // Get the size of the loop device
        let output = Command::new("blockdev")
            .args(["--getsize64", &device.path_str()])
            .output()
            .await
            .map_err(|e| LvmError::PhysicalVolumeCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::PhysicalVolumeCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let size_bytes = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .map_err(|e| LvmError::PhysicalVolumeCreate(e.to_string()))?;

        debug!(size = size_bytes, "Creating physical volume");

        let output = Command::new("pvcreate")
            .args([
                "--setphysicalvolumesize",
                &format!("{}b", size_bytes),
                &device.path_str(),
            ])
            .output()
            .await
            .map_err(|e| LvmError::PhysicalVolumeCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::PhysicalVolumeCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        debug!(device_path = ?device.path(), "Physical volume initialized");

        Ok(Self { device })
    }

    pub fn from_existing(device: LoopDevice) -> Result<Self, LvmError> {
        match device.path().exists() {
            true => Ok(Self { device }),
            false => Err(LvmError::PhysicalVolumeFromExisting(device.path_str())),
        }
    }

    /// Returns the path to the device on which this volume has been initialized, eg: /dev/loop4
    pub fn path(&self) -> PathBuf {
        self.device.path()
    }

    pub fn path_str(&self) -> String {
        self.path().display().to_string()
    }

    pub fn delete(&self) -> Result<(), LvmError> {
        let path = self.path_str();
        debug!(path = %path, "Deleting physical volume");

        let output = std::process::Command::new("pvremove")
            .args(["-y", &path])
            .output()
            .map_err(|e| {
                debug!(path = %path, error = %e, "Failed to execute pvremove command");
                LvmError::PhysicalVolumeDelete(format!("{}: {}", path, e.to_string()))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            debug!(path = %path, error = %error, "pvremove command failed");
            return Err(LvmError::PhysicalVolumeDelete(format!(
                "{}: {}",
                path, error
            )));
        }

        debug!(path = %path, "Successfully deleted physical volume");
        self.device.delete()
    }
}
