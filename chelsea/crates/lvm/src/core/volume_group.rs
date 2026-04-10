use serde::{Deserialize, Serialize};
use tracing::debug;

use super::physical_volume::PhysicalVolume;
use crate::error::LvmError;
use std::path::PathBuf;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeGroupCreateOptions {
    pub name: String,
}

impl Default for VolumeGroupCreateOptions {
    /// Note that due to the way LayeredFsOptions are created, these defaults may differ from those of LayeredFsOptions::default()
    fn default() -> Self {
        Self {
            name: "chelsea".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct VolumeGroup {
    name: String,
    /// A list of volumes used to create this group
    pub volumes: Vec<PhysicalVolume>,
}

impl VolumeGroup {
    pub async fn new(
        volumes: Vec<PhysicalVolume>,
        options: VolumeGroupCreateOptions,
    ) -> Result<Self, LvmError> {
        let volume_paths: Vec<_> = volumes.iter().map(|v| v.path()).collect();

        let output = Command::new("vgcreate")
            .arg(&options.name)
            .args(&volume_paths)
            .output()
            .await
            .map_err(|e| LvmError::VolumeGroupCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::VolumeGroupCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        debug!(
            name = options.name,
            volumes = ?volume_paths,
            "Volume group created",
        );

        Ok(Self {
            name: options.name,
            volumes,
        })
    }

    pub fn from_existing(name: String, volumes: Vec<PhysicalVolume>) -> Result<Self, LvmError> {
        let output = std::process::Command::new("vgdisplay")
            .arg(&name)
            .output()
            .map_err(|e| LvmError::VolumeGroupFromExisting(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::VolumeGroupFromExisting(name));
        }

        Ok(Self { name, volumes })
    }

    /// Returns the full path to the volume group, eg: /dev/vg
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!("/dev/{}", self.name))
    }

    pub fn path_str(&self) -> String {
        self.path().display().to_string()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn delete(&self) -> Result<(), LvmError> {
        debug!(name = %self.name, "Deleting volume group");

        let output = std::process::Command::new("vgremove")
            .args(["-y", &self.name])
            .output()
            .map_err(|e| {
                debug!(name = %self.name, error = %e, "Failed to execute vgremove command");
                LvmError::VolumeGroupDelete(vec![format!("{}: {}", self.name, e.to_string())])
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            debug!(name = %self.name, error = %error, "vgremove command failed");
            return Err(LvmError::VolumeGroupDelete(vec![format!(
                "{}: {}",
                self.name, error
            )]));
        }

        debug!(name = %self.name, "Successfully deleted volume group");
        debug!(name = %self.name, "Deleting physical volumes");

        let errors: Vec<LvmError> = self
            .volumes
            .iter()
            .map(|v| v.delete())
            .filter_map(|e| e.err())
            .collect();

        if !errors.is_empty() {
            debug!(name = %self.name, errors = ?errors, "Failed to delete some physical volumes");
            return Err(LvmError::VolumeGroupDelete(
                errors.iter().map(|e| e.to_string()).collect(),
            ));
        }

        debug!(name = %self.name, "Successfully deleted all physical volumes");
        Ok(())
    }
}
