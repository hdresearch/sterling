// thin_pool.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

use super::{thin_volume::ThinVolume, volume_group::VolumeGroup};
use crate::error::LvmError;
use std::path::PathBuf;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinPoolCreateOptions {
    pub name: String,
}

impl Default for ThinPoolCreateOptions {
    fn default() -> Self {
        Self {
            name: "pool".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct ThinPool {
    /// The pool's name, without any prefixes, eg: "pool"
    pub name: String,
    /// The volume group backing the pool
    pub volume_group: VolumeGroup,
    /// All volumes in this pool
    pub volumes: Mutex<HashMap<String, Arc<ThinVolume>>>,
}

impl ThinPool {
    pub async fn new(
        volume_group: VolumeGroup,
        options: ThinPoolCreateOptions,
    ) -> Result<Self, LvmError> {
        let group_name = volume_group.name();

        let output = Command::new("lvcreate")
            .args([
                "-l",
                "100%FREE",
                "-T",
                &format!("{}/{}", group_name, options.name),
            ])
            .output()
            .await
            .map_err(|e| LvmError::ThinPoolCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::ThinPoolCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let new_pool = Self {
            name: options.name,
            volume_group,
            volumes: Mutex::new(HashMap::new()),
        };

        debug!(path = %new_pool.path_str(), "Thin pool created");

        Ok(new_pool)
    }

    pub fn from_existing(name: String, volume_group: VolumeGroup) -> Result<Self, LvmError> {
        let output = std::process::Command::new("vgs")
            .args(["--noheadings", "-o", "vg_name", &volume_group.name()])
            .output()
            .map_err(|e| {
                LvmError::ThinPoolFromExisting(format!(
                    "Volume group {} not found: {e}",
                    volume_group.name()
                ))
            })?;

        if output.status.success() {
            Ok(Self {
                name,
                volume_group,
                volumes: Mutex::new(HashMap::new()),
            })
        } else {
            Err(LvmError::ThinPoolFromExisting(format!(
                "Volume group {} not found: {}",
                volume_group.name(),
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    /// Returns the full path to the pool, eg: /dev/vg/pool
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!("{}/{}", self.volume_group.path_str(), self.name))
    }

    pub fn path_str(&self) -> String {
        self.path().display().to_string()
    }

    /// Returns the full pool name, eg: vg/pool
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.volume_group.name(), self.name)
    }

    pub async fn add_volume(&self, volume: Arc<ThinVolume>) {
        self.volumes.lock().await.insert(volume.id.clone(), volume);
    }

    pub async fn get_volume(&self, volume_id: &str) -> Option<Arc<ThinVolume>> {
        self.volumes.lock().await.get(volume_id).cloned()
    }

    pub async fn delete(&self) -> Result<(), LvmError> {
        debug!("Deleting all volumes in pool");
        for (_, volume) in self.volumes.lock().await.drain() {
            debug!(volume_id = %volume.id, "Deleting volume");
            if let Err(e) = volume.delete().await {
                tracing::error!(volume_id = %volume.id, error = %e, "Failed to delete volume");
            }
        }

        // Then delete the pool itself
        let path = self.path_str();
        debug!(path = %path, "Deleting thin pool");
        let output = std::process::Command::new("lvremove")
            .args(["-y", &path])
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            debug!(path = %path, error = %error, "Failed to delete thin pool");
            return Err(LvmError::ThinPoolDelete(format!("{}: {}", path, error)));
        }

        debug!(path = %path, "Successfully deleted thin pool");
        self.volume_group.delete()
    }
}
