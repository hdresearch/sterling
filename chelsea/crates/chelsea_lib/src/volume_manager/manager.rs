use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;
use vers_pg::schema::chelsea::tables::sleep_snapshot::RecordVolumeSleepSnapshot;

use crate::{
    volume::VmVolume,
    volume_manager::{
        VmVolumeCommitMetadata, error::CreateVmVolumeFromImageError,
        sleep_snapshot::VmVolumeSleepSnapshotMetadata,
    },
};

/// A trait that must be implemented by all VmVolumeManager variants.
#[async_trait]
pub trait VmVolumeManager: Send + Sync {
    /// Get the size of a base image in MiB without creating a volume
    async fn get_base_image_size_mib(
        &self,
        image_name: &str,
    ) -> Result<u32, CreateVmVolumeFromImageError>;
    /// If None, use existing image size as VM size.
    async fn create_volume_from_base_image(
        &self,
        image_name: String,
        vm_volume_size_mib: u32,
    ) -> Result<Arc<dyn VmVolume>, CreateVmVolumeFromImageError>;
    async fn create_volume_from_volume(
        &self,
        volume_id: &Uuid,
    ) -> anyhow::Result<Arc<dyn VmVolume>>;
    async fn rehydrate_vm_volume(&self, volume_id: &Uuid) -> anyhow::Result<Arc<dyn VmVolume>>;
    /// Commits the volume, returning a vec of file names created in `commit_dir`
    async fn commit_volume(
        &self,
        volume_id: &Uuid,
        commit_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, VmVolumeCommitMetadata)>;
    /// Calculate (or estimate) the disk space required to write commit files
    fn calculate_commit_size_mib(&self, volume_id: &Uuid) -> u32;
    /// Sleep snapshots the volume, returning a vec of file names created in `snapshot_dir`
    async fn sleep_snapshot_volume(
        &self,
        volume_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, VmVolumeSleepSnapshotMetadata)>;
    /// Calculate (or estimate) the disk space required to write sleep snapshot files
    fn calculate_sleep_snapshot_size_mib(&self, volume_id: &Uuid) -> u32;
    /// Creates a volume, using the information previously stored in a VmVolumeCommitMetadata object to do so.
    async fn create_volume_from_commit_metadata(
        &self,
        volume_commit_metadata: &VmVolumeCommitMetadata,
    ) -> anyhow::Result<Arc<dyn VmVolume>>;
    /// Creates a volume, using the information previously stored in a RecordVolumeSleepSnapshot object to do so.
    async fn create_volume_from_sleep_snapshot_record(
        &self,
        volume_sleep_snapshot_record: &RecordVolumeSleepSnapshot,
    ) -> anyhow::Result<Arc<dyn VmVolume>>;
    /// Resize the requested volume (full offline resize: rbd + fsck + resize2fs).
    /// Only safe when the device is not in use by a running/paused VM.
    async fn resize_volume(
        &self,
        vm_volume_id: &Uuid,
        vm_volume_size_mib: u32,
    ) -> anyhow::Result<()>;
    /// Resize only the underlying block device without touching the filesystem.
    /// Safe to call while the device is attached to a running/paused VM.
    /// The guest must run resize2fs itself after resume.
    async fn resize_volume_device_only(
        &self,
        vm_volume_id: &Uuid,
        vm_volume_size_mib: u32,
    ) -> anyhow::Result<()>;
    /// Callback invoked when a VmVolume's parent VM is killed
    async fn on_vm_killed(&self, vm_volume_id: &Uuid) -> anyhow::Result<()>;
    /// Callback invoked when a VmVolume's parent VM is put to sleep
    async fn on_vm_sleep(&self, vm_volume_id: &Uuid) -> anyhow::Result<()>;
    /// Callback to be invoked when a VmVolume's parent VM is resumed
    async fn on_vm_resumed(&self, vm_volume_id: &Uuid) -> anyhow::Result<()>;
}
