use std::path::PathBuf;

use async_trait::async_trait;
use uuid::Uuid;

/// A trait that must be implemented by all VmVolume variants.
#[async_trait]
pub trait VmVolume: Sync + Send {
    /// Returns the absolute device path
    fn path(&self) -> PathBuf;
    /// Returns the volume's ID, a (v4) UUID
    fn id(&self) -> Uuid;
    /// Returns the volume's image name, a (v4) UUID
    fn image_name(&self) -> String;
    /// Deletes the volume
    async fn delete(&self) -> anyhow::Result<()>;
    /// Grow the volume to the requested size. Returns error if the requested size is smaller than current.
    /// Performs a full offline resize (rbd + fsck + resize2fs). Only safe when the device is not in use.
    async fn grow(&self, vm_volume_size_mib: u32) -> anyhow::Result<()>;
    /// Grow only the underlying block device without touching the filesystem.
    /// Safe to call while the device is attached to a running/paused VM.
    async fn grow_device_only(&self, vm_volume_size_mib: u32) -> anyhow::Result<()>;
}
