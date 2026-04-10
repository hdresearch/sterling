use serde::{Deserialize, Serialize};
use std::sync::{Arc, Weak};
use tracing::debug;
use util::linux::{dd, DDOptions};
use uuid::Uuid;

use super::thin_pool::ThinPool;
use crate::error::LvmError;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootVolumeCreateOptions {
    /// The name of the root volume. Default: root
    pub name: String,
    /// The virtual size of the volume, in bytes. Default 536870912 (512 MB)
    pub size_mib: u32,
    /// Whether or not to format the volume with an empty ext4 filesystem
    pub should_format: bool,
}

impl RootVolumeCreateOptions {
    pub fn size_bytes(&self) -> u64 {
        self.size_mib as u64 * 1024 * 1024
    }
}

impl Default for RootVolumeCreateOptions {
    fn default() -> Self {
        Self {
            name: "root".to_string(),
            size_mib: 1 * 1024,
            should_format: true,
        }
    }
}

#[derive(Debug)]
pub struct ThinVolume {
    /// UUID
    pub id: String,
    /// The volume name, eg: vm3
    pub name: String,
    /// A weak pointer to the containing pool
    thin_pool: Weak<ThinPool>,
}

impl ThinVolume {
    /// This constructor creates a new thin volume on the given thin pool and optionally formats it with an ext4 filesystem.
    /// This is used for creating the "root volume" for a LayeredFs.
    pub async fn new_root(
        thin_pool: Arc<ThinPool>,
        options: RootVolumeCreateOptions,
    ) -> Result<Arc<Self>, LvmError> {
        let pool_full_name = thin_pool.full_name();

        // Create the new volume
        let output = Command::new("lvcreate")
            .args([
                "-V",
                format!("{}m", &options.size_mib.to_string()).as_str(),
                "-T",
                &pool_full_name,
                "-n",
                &options.name,
            ])
            .output()
            .await
            .map_err(|e| LvmError::ThinVolumeCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::ThinVolumeCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let new_volume = Arc::new(Self {
            id: Uuid::new_v4().to_string(),
            name: options.name.clone(),
            thin_pool: Arc::downgrade(&thin_pool),
        });

        // Format the volume with an ext4 filesystem if requested
        if options.should_format {
            debug!(
                path = ?new_volume.path(),
                size_mib = options.size_mib,
                "Formatting volume with an ext4 filesystem",
            );
            let output = Command::new("mkfs.ext4")
                .args([new_volume.path_str()?])
                .output()
                .await
                .map_err(|e| LvmError::ThinVolumeMkfs(e.to_string()))?;

            if !output.status.success() {
                return Err(LvmError::ThinVolumeMkfs(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ));
            }
        }

        debug!(path = ?new_volume.path(), "Created logical volume");

        // Add the volume to the containing pool
        thin_pool.add_volume(new_volume.clone()).await;

        Ok(new_volume)
    }

    pub fn from_existing(
        id: String,
        name: String,
        thin_pool: Weak<ThinPool>,
    ) -> Result<Self, LvmError> {
        let pool = thin_pool
            .upgrade()
            .ok_or_else(|| LvmError::ThinVolumeFromExisting("Pool no longer exists".to_string()))?;

        match pool.volume_group.path().join(&name).exists() {
            true => Ok(Self {
                id,
                name,
                thin_pool,
            }),
            false => Err(LvmError::ThinVolumeFromExisting(name)),
        }
    }

    /// Returns the full path to the volume, eg: /dev/vg/vm3
    pub fn path(&self) -> Result<PathBuf, LvmError> {
        self.thin_pool
            .upgrade()
            .ok_or_else(|| LvmError::ThinVolumeParentUpgrade(self.id.clone()))
            .map(|pool| pool.volume_group.path().join(&self.name))
    }

    pub fn path_str(&self) -> Result<String, LvmError> {
        self.path().map(|p| p.display().to_string())
    }

    /// Get a reference to the thin pool (if it still exists)
    pub fn thin_pool(&self) -> Result<Arc<ThinPool>, LvmError> {
        self.thin_pool
            .upgrade()
            .ok_or_else(|| LvmError::ThinVolumeParentUpgrade(self.id.clone()))
    }

    /// Creates a snapshot of this volume.
    /// If name is provided, uses that as the snapshot name.
    /// Otherwise generates a name using the pattern "{original_name}_snap{number}".
    pub async fn snapshot(&self, name: Option<String>) -> Result<Arc<Self>, LvmError> {
        let pool = self.thin_pool()?;

        debug!(name = ?name, "Snapshotting volume with new name");
        let snapshot_name = name.unwrap_or_else(|| Uuid::new_v4().to_string());

        let source_path = self.path_str()?;

        // Create the snapshot
        let output = Command::new("lvcreate")
            .args([
                "-s",
                "-kn",
                "-ay",
                &source_path,
                "-n",
                snapshot_name.as_str(),
            ])
            .output()
            .await
            .map_err(|e| LvmError::ThinVolumeCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::ThinVolumeCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        debug!(snapshot_name, path = ?self.path(), "Snapshot created");

        let child = Arc::new(Self {
            id: Uuid::new_v4().to_string(),
            name: snapshot_name,
            thin_pool: self.thin_pool.clone(),
        });

        // Add the snapshot to the containing pool
        pool.add_volume(child.clone()).await;

        Ok(child)
    }

    pub async fn archive(&self, dest: &Path) -> Result<(), LvmError> {
        // Create a snapshot of the volume first
        let snapshot = self.snapshot(None).await.map_err(|e| {
            LvmError::ThinVolumeArchive(format!("Failed to create snapshot: {}", e))
        })?;

        let snapshot_path = snapshot.path()?;

        debug!(
            source = ?snapshot_path,
            destination = ?dest,
            "Creating disk image using dd"
        );

        // Create the disk image using dd
        dd(&snapshot_path, dest, DDOptions::default())
            .await
            .map_err(|e| {
                LvmError::ThinVolumeArchive(format!("Failed to create disk image: {}", e))
            })?;

        Ok(())
    }

    pub async fn delete(&self) -> Result<(), LvmError> {
        let path_str = self.path_str()?;
        debug!(path = %path_str, "Deleting thin volume");

        let output = std::process::Command::new("lvremove")
            .args(["-y", &path_str])
            .output()
            .map_err(|e| LvmError::ThinVolumeDelete(e.to_string()))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            debug!(error = %error, "Failed to delete thin volume");
            return Err(LvmError::ThinVolumeDelete(error));
        }

        debug!("Successfully deleted thin volume");

        Ok(())
    }
}
