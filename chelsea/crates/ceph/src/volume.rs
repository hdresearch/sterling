use std::path::{Path, PathBuf};

use anyhow::anyhow;
use tokio::process::Command;
use tracing::debug;
use util::linux::{DDOptions, dd, fsck_force_yes, resize2fs};
use uuid::Uuid;

use crate::{RbdClientError, RbdSnapName, default_rbd_client};

#[derive(Debug)]
pub struct ThinVolume {
    pub id: Uuid,
    pub image_name: String,
    pub device_path: PathBuf,
}

impl ThinVolume {
    /// Create a new image on RBD and map it to a block device
    pub async fn new_mapped(id: Uuid, size_mib: u32) -> Result<Self, RbdClientError> {
        debug!(%id, size_mib, "Creating new ThinVolume");
        let client = default_rbd_client()?;
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

    /// Create a new image based on the given snap and map it to a block device
    pub async fn new_mapped_from_snap(
        id: Uuid,
        snap_name: &RbdSnapName,
    ) -> Result<Self, RbdClientError> {
        debug!(%id, %snap_name, "Creating new ThinVolume");
        let client = default_rbd_client()?;
        let image_name = Uuid::new_v4().to_string();

        client.snap_clone(snap_name, &image_name).await?;
        let device_path = client.device_map(&image_name).await?;

        let volume = Self {
            id,
            image_name,
            device_path,
        };

        debug!(?volume, "Created new ThinVolume");
        Ok(volume)
    }

    /// Map the given image to a block device
    pub async fn new_mapped_from_image(
        id: Uuid,
        image_name: impl AsRef<str>,
    ) -> Result<Self, RbdClientError> {
        let image_name = image_name.as_ref();

        debug!(%id, %image_name, "Creating new ThinVolume");
        let client = default_rbd_client()?;

        let device_path = client.device_map(image_name).await?;

        let volume = Self {
            id,
            image_name: image_name.to_string(),
            device_path,
        };

        debug!(?volume, "Created new ThinVolume");
        Ok(volume)
    }

    /// Construct a ThinVolume from known parameters. Note that this has no side effects or validation
    pub fn from_existing(id: Uuid, image_name: String, device_path: PathBuf) -> Self {
        Self {
            id,
            image_name,
            device_path,
        }
    }

    pub fn path<'a>(&'a self) -> &'a Path {
        self.device_path.as_path()
    }

    pub fn path_str(&self) -> String {
        self.device_path.to_string_lossy().to_string()
    }

    pub async fn create_snap(&self) -> Result<RbdSnapName, RbdClientError> {
        let snap_name = RbdSnapName {
            image_name: self.image_name.clone(),
            snap_name: Uuid::new_v4().to_string(),
        };
        debug!(%snap_name, volume_id = %self.id, "Creating new ThinVolume snap");

        let client = default_rbd_client()?;

        client.snap_create(&snap_name).await?;
        client.snap_protect(&snap_name).await?;

        Ok(snap_name)
    }

    pub async fn delete_snap(&self, snap_name: String) -> Result<(), RbdClientError> {
        let snap_name = &RbdSnapName {
            image_name: self.image_name.clone(),
            snap_name,
        };

        debug!(%snap_name, volume_id = %self.id, "Deleting ThinVolume snap");
        let client = default_rbd_client()?;

        client.snap_unprotect(snap_name).await?;
        client.snap_remove(snap_name).await?;

        Ok(())
    }

    /// Creates a new ThinVolume based on the provided snap name; if the snap name is not found, an error will be returned. Tip: If you want to use the most recent snap, use
    /// ThinVolume::get_or_create_current_snap().
    pub async fn create_child_mapped(
        &self,
        child_id: Uuid,
        snap_name: &RbdSnapName,
    ) -> Result<Self, RbdClientError> {
        debug!(%child_id, %snap_name, "Creating new child ThinVolume");
        let child_image_name = Uuid::new_v4().to_string();

        let client = default_rbd_client()?;
        client.snap_clone(snap_name, &child_image_name).await?;
        let child_device_path = client.device_map(&child_image_name).await?;

        let child_volume = Self {
            id: child_id,
            image_name: child_image_name,
            device_path: child_device_path,
        };
        debug!(?child_volume, "Created new child ThinVolume");
        Ok(child_volume)
    }

    /// Attempt to unmap the ThinVolume. Note that this will not delete the image or snaps that depend on it - these may be committed and need to be handled separately.
    pub async fn delete(&self) -> Result<(), RbdClientError> {
        debug!(id = %self.id, "Deleting ThinVolume");

        let client = default_rbd_client()?;
        client.device_unmap(&self.device_path).await
    }

    pub async fn dd<P: AsRef<Path>>(&self, dest: P) -> anyhow::Result<()> {
        dd(self.path(), dest, DDOptions::default()).await
    }

    // pub async fn as_db_schema(&self) -> chelsea_db::schema::ThinVolume {
    //     chelsea_db::schema::ThinVolume {
    //         id: self.id.clone(),
    //         size_mib: self.size.to_mib(),
    //         image_name: self.image_name.clone(),
    //         device_path: self.device_path.display().to_string(),
    //         current_snap: self.get_current_snap().await.map(|s| s.to_string()),
    //     }
    // }

    pub async fn list_snaps(&self) -> Result<Vec<RbdSnapName>, RbdClientError> {
        let client = default_rbd_client()?;
        client.snap_list(&self.image_name).await
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

    /// Creates an image, getting or creating the current snap to do so. Returns the image name.
    pub async fn create_snap_then_image(&self) -> Result<String, RbdClientError> {
        let client = default_rbd_client()?;

        let snap_name = self.create_snap().await?;

        let image_name = Uuid::new_v4().to_string();
        client.snap_clone(&snap_name, &image_name).await?;

        Ok(image_name)
    }

    /// Grow a volume. Note that shrinking is unsupported.
    /// This performs a full offline resize: rbd resize + fsck + resize2fs on the host.
    /// Only safe when the device is NOT in use by a running/paused VM.
    pub async fn grow(&self, size_mib: u32) -> Result<(), RbdClientError> {
        default_rbd_client()?
            .image_grow(&self.image_name, size_mib)
            .await?;
        fsck_force_yes(self.path())
            .await
            .map_err(|e| RbdClientError::Other(e.to_string()))?;
        resize2fs(self.path())
            .await
            .map_err(|e| RbdClientError::Other(e.to_string()))?;

        Ok(())
    }

    /// Grow only the underlying RBD block device without touching the filesystem.
    /// Use this when the device is attached to a running/paused VM — the guest kernel
    /// owns the mounted filesystem and must run resize2fs itself (online resize).
    pub async fn grow_device_only(&self, size_mib: u32) -> Result<(), RbdClientError> {
        default_rbd_client()?
            .image_grow(&self.image_name, size_mib)
            .await
    }

    pub async fn get_size_mib(&self) -> Result<u32, RbdClientError> {
        default_rbd_client()?
            .image_info(&self.image_name)
            .await
            .map(|info| info.size_mib())
    }
}
