//! High-level volume abstraction over RBD images.
//!
//! A `ThinVolume` represents a mapped RBD image with snapshot and
//! lifecycle management. This is a port of the old `ceph::ThinVolume`
//! that uses the native cephalopod `Client` instead of shelling out.

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use tokio::process::Command;
use tracing::debug;
use util::linux::{DDOptions, dd, fsck_force_yes, resize2fs};
use uuid::Uuid;

use crate::default::default_client;
use crate::error::CephalopodError;
use crate::snap_name::RbdSnapName;

#[derive(Debug)]
pub struct ThinVolume {
    pub id: Uuid,
    pub image_name: String,
    pub device_path: PathBuf,
}

impl ThinVolume {
    /// Create a new image on RBD and map it to a block device.
    pub async fn new_mapped(id: Uuid, size_mib: u32) -> Result<Self, CephalopodError> {
        debug!(%id, size_mib, "Creating new ThinVolume");
        let client = default_client()?;
        let image_name = Uuid::new_v4().to_string();
        client.image_create(&image_name, size_mib).await?;
        let device_path = client.device_map(&image_name).await?;

        let volume = Self {
            id,
            image_name,
            device_path,
        };
        debug!(?volume, "Created new ThinVolume");
        Ok(volume)
    }

    /// Create a new image based on the given snap and map it to a block device.
    pub async fn new_mapped_from_snap(
        id: Uuid,
        snap_name: &RbdSnapName,
    ) -> Result<Self, CephalopodError> {
        debug!(%id, %snap_name, "Creating new ThinVolume from snap");
        let client = default_client()?;
        let image_name = Uuid::new_v4().to_string();

        client.snap_clone_named(snap_name, &image_name).await?;
        let device_path = client.device_map(&image_name).await?;

        let volume = Self {
            id,
            image_name,
            device_path,
        };
        debug!(?volume, "Created new ThinVolume");
        Ok(volume)
    }

    /// Map an existing image to a block device.
    pub async fn new_mapped_from_image(
        id: Uuid,
        image_name: impl AsRef<str>,
    ) -> Result<Self, CephalopodError> {
        let image_name = image_name.as_ref();
        debug!(%id, %image_name, "Creating new ThinVolume from image");
        let client = default_client()?;

        let device_path = client.device_map(image_name).await?;

        let volume = Self {
            id,
            image_name: image_name.to_string(),
            device_path,
        };
        debug!(?volume, "Created new ThinVolume");
        Ok(volume)
    }

    /// Construct a ThinVolume from known parameters. No side effects or validation.
    pub fn from_existing(id: Uuid, image_name: String, device_path: PathBuf) -> Self {
        Self {
            id,
            image_name,
            device_path,
        }
    }

    pub fn path(&self) -> &Path {
        self.device_path.as_path()
    }

    pub fn path_str(&self) -> String {
        self.device_path.to_string_lossy().to_string()
    }

    /// Create a snapshot of this volume. The snapshot is automatically protected.
    pub async fn create_snap(&self) -> Result<RbdSnapName, CephalopodError> {
        let snap_name = RbdSnapName {
            image_name: self.image_name.clone(),
            snap_name: Uuid::new_v4().to_string(),
        };
        debug!(%snap_name, volume_id = %self.id, "Creating new ThinVolume snap");

        let client = default_client()?;
        client.snap_create_named(&snap_name).await?;
        client.snap_protect_named(&snap_name).await?;

        Ok(snap_name)
    }

    /// Delete a snapshot (unprotects first).
    pub async fn delete_snap(&self, snap_name: String) -> Result<(), CephalopodError> {
        let snap = RbdSnapName {
            image_name: self.image_name.clone(),
            snap_name,
        };
        debug!(%snap, volume_id = %self.id, "Deleting ThinVolume snap");

        let client = default_client()?;
        client.snap_unprotect_named(&snap).await?;
        client.snap_remove_named(&snap).await?;

        Ok(())
    }

    /// Create a child volume from a snapshot.
    pub async fn create_child_mapped(
        &self,
        child_id: Uuid,
        snap_name: &RbdSnapName,
    ) -> Result<Self, CephalopodError> {
        debug!(%child_id, %snap_name, "Creating new child ThinVolume");
        let child_image_name = Uuid::new_v4().to_string();

        let client = default_client()?;
        client
            .snap_clone_named(snap_name, &child_image_name)
            .await?;
        let child_device_path = client.device_map(&child_image_name).await?;

        let child_volume = Self {
            id: child_id,
            image_name: child_image_name,
            device_path: child_device_path,
        };
        debug!(?child_volume, "Created new child ThinVolume");
        Ok(child_volume)
    }

    /// Unmap the volume. Does not delete the image or its snapshots.
    pub async fn delete(&self) -> Result<(), CephalopodError> {
        debug!(id = %self.id, "Deleting ThinVolume");
        let client = default_client()?;
        client.device_unmap(&self.device_path).await
    }

    pub async fn dd<P: AsRef<Path>>(&self, dest: P) -> anyhow::Result<()> {
        dd(self.path(), dest, DDOptions::default()).await
    }

    /// List all snapshots on this volume.
    pub async fn list_snaps(&self) -> Result<Vec<RbdSnapName>, CephalopodError> {
        let client = default_client()?;
        client.snap_list_named(&self.image_name).await
    }

    pub async fn mkfs_ext4(&self) -> anyhow::Result<()> {
        let output = Command::new("mkfs.ext4")
            .arg(&self.device_path)
            .output()
            .await?;

        match output.status.success() {
            true => Ok(()),
            false => Err(anyhow!(
                "mkfs.ext4 failed on device: {}\nstdout: {}\nstderr: {}",
                self.device_path.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )),
        }
    }

    /// Create a snapshot then clone it to a new image. Returns the new image name.
    pub async fn create_snap_then_image(&self) -> Result<String, CephalopodError> {
        let client = default_client()?;
        let snap_name = self.create_snap().await?;

        let image_name = Uuid::new_v4().to_string();
        client.snap_clone_named(&snap_name, &image_name).await?;

        Ok(image_name)
    }

    /// Grow a volume (offline). Performs rbd resize + fsck + resize2fs.
    /// Only safe when the device is NOT in use by a running/paused VM.
    pub async fn grow(&self, size_mib: u32) -> Result<(), CephalopodError> {
        default_client()?
            .image_grow(&self.image_name, size_mib)
            .await?;
        fsck_force_yes(self.path())
            .await
            .map_err(|e| CephalopodError::Device(e.to_string()))?;
        resize2fs(self.path())
            .await
            .map_err(|e| CephalopodError::Device(e.to_string()))?;

        Ok(())
    }

    /// Grow only the underlying RBD block device without touching the filesystem.
    /// Use when the device is attached to a running/paused VM.
    pub async fn grow_device_only(&self, size_mib: u32) -> Result<(), CephalopodError> {
        default_client()?
            .image_grow(&self.image_name, size_mib)
            .await
    }

    pub async fn get_size_mib(&self) -> Result<u32, CephalopodError> {
        default_client()?
            .image_info(&self.image_name)
            .await
            .map(|info| info.size_mib())
    }
}
