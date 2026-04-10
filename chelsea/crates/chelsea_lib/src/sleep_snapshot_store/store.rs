use async_trait::async_trait;
use std::path::Path;
use uuid::Uuid;
use vers_pg::schema::chelsea::tables::sleep_snapshot::SleepSnapshotFile;

#[async_trait]
/// Represents an interface for storing sleep snapshot files on some remote target, with a local cache
pub trait VmSleepSnapshotStore: Send + Sync {
    /// Upload the provided files to the store, returning a Vec representing the files on the store for later retrieval.
    async fn upload_sleep_snapshot_files(
        &self,
        sleep_snapshot_id: &Uuid,
        to_upload: &[String],
    ) -> anyhow::Result<Vec<SleepSnapshotFile>>;

    /// Retrieve the given files from the store, placing them into the sleep_snapshots subdir of the data directory.
    async fn download_sleep_snapshot_files(
        &self,
        to_download: &[SleepSnapshotFile],
    ) -> anyhow::Result<()>;

    /// The local path which is used for sleep_snapshot file uploads+downloads
    fn sleep_snapshot_dir<'a>(&'a self) -> &'a Path;

    /// Ensure there is at least {required_size_mib} of space available in the sleep_snapshot cache
    async fn ensure_space(&self, space_required_mib: u32) -> anyhow::Result<()>;

    /// Delete the specified files from the store, eg: after waking the VM
    async fn delete_sleep_snapshot_files(
        &self,
        to_delete: &[SleepSnapshotFile],
    ) -> anyhow::Result<()>;
}
