use std::{path::Path, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_postgres::{Client, Row, Statement};
use tracing::warn;
use uuid::Uuid;

type PgResult<T> = Result<T, crate::Error>;

/// chelsea.sleep_snapshot table
pub struct TableSleepSnapshot {
    client: Arc<Client>,
    stmt_fetch_by_vm_id: Statement,
    stmt_insert: Statement,
    stmt_soft_delete_by_id: Statement,
    stmt_image_name_exists: Statement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// chelsea.sleep_snapshot record
pub struct RecordSleepSnapshot {
    pub id: Uuid,
    pub vm_id: Uuid,
    pub host_architecture: String,
    pub kernel_name: String,
    pub base_image: String,
    pub vcpu_count: u32,
    pub mem_size_mib: u32,
    pub fs_size_mib: u32,
    pub ssh_public_key: String,
    pub ssh_private_key: String,
    pub process_sleep_snapshot: RecordProcessSleepSnapshot,
    pub volume_sleep_snapshot: RecordVolumeSleepSnapshot,
    pub remote_files: Vec<SleepSnapshotFile>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordProcessSleepSnapshot {
    Firecracker(RecordFirecrackerProcessSleepSnapshot),
    CloudHypervisor(RecordCloudHypervisorProcessSleepSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordFirecrackerProcessSleepSnapshot {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordCloudHypervisorProcessSleepSnapshot {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordVolumeSleepSnapshot {
    Ceph(RecordCephVolumeSleepSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordCephVolumeSleepSnapshot {
    pub image_name: String,
}

impl TryFrom<Row> for RecordSleepSnapshot {
    type Error = crate::Error;
    fn try_from(value: Row) -> Result<Self, Self::Error> {
        Ok(RecordSleepSnapshot {
            id: value.try_get("id")?,
            vm_id: value.try_get("vm_id")?,
            host_architecture: value.try_get("host_architecture")?,
            kernel_name: value.try_get("kernel_name")?,
            base_image: value.try_get("base_image")?,
            vcpu_count: value.try_get("vcpu_count")?,
            mem_size_mib: value.try_get("mem_size_mib")?,
            fs_size_mib: value.try_get("fs_size_mib")?,
            ssh_public_key: value.try_get("ssh_public_key")?,
            ssh_private_key: value.try_get("ssh_private_key")?,
            process_sleep_snapshot: serde_json::from_value(
                value.try_get::<_, Value>("process_sleep_snapshot")?,
            )?,
            volume_sleep_snapshot: serde_json::from_value(
                value.try_get::<_, Value>("volume_sleep_snapshot")?,
            )?,
            remote_files: serde_json::from_value(value.try_get("remote_files")?)?,
            created_at: value.try_get("created_at")?,
            deleted_at: value.try_get("deleted_at")?,
        })
    }
}

/// Information about a file uploaded to the VmSleepSnapshotStore
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SleepSnapshotFile {
    /// The file's key on the VmSleepSnapshotStore. Should include prefixes.
    pub key: String,
    // /// The file checksum.
    // TODO: Uncomment and implement
    // pub checksum: String,
}

impl SleepSnapshotFile {
    /// Compute the filename from the file key, stripping its prefix
    pub fn file_name<'a>(&'a self) -> Result<&'a str, String> {
        Path::new(&self.key)
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .ok_or(format!("Failed to extract file name from key {}", self.key))
    }
}

impl ToString for SleepSnapshotFile {
    fn to_string(&self) -> String {
        self.key.clone()
    }
}

impl From<String> for SleepSnapshotFile {
    fn from(value: String) -> Self {
        Self { key: value }
    }
}

impl TableSleepSnapshot {
    pub async fn new(client: Arc<Client>) -> PgResult<Self> {
        Ok(Self {
            stmt_fetch_by_vm_id: client
                .prepare(
                    "SELECT * FROM chelsea.sleep_snapshot \
                     WHERE vm_id = $1 AND deleted_at IS NULL \
                     ORDER BY created_at DESC",
                )
                .await?,
            stmt_insert: client
                .prepare(
                    "INSERT INTO chelsea.sleep_snapshot \
                     (id, vm_id, host_architecture, kernel_name, base_image, vcpu_count, \
                      mem_size_mib, fs_size_mib, ssh_public_key, ssh_private_key, \
                      process_sleep_snapshot, volume_sleep_snapshot, remote_files) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
                )
                .await?,
            stmt_soft_delete_by_id: client
                .prepare("UPDATE chelsea.sleep_snapshot SET deleted_at = NOW() WHERE id = $1")
                .await?,
            stmt_image_name_exists: client
                .prepare(
                    "SELECT COUNT(*) FROM chelsea.sleep_snapshot \
                     WHERE volume_sleep_snapshot->'Ceph'->>'image_name' = $1",
                )
                .await?,
            client,
        })
    }

    pub async fn insert(
        &self,
        id: &Uuid,
        vm_id: &Uuid,
        host_architecture: &str,
        kernel_name: &str,
        base_image: &str,
        vcpu_count: u32,
        mem_size_mib: u32,
        fs_size_mib: u32,
        ssh_public_key: &str,
        ssh_private_key: &str,
        process_sleep_snapshot: &RecordProcessSleepSnapshot,
        volume_sleep_snapshot: &RecordVolumeSleepSnapshot,
        remote_files: &[SleepSnapshotFile],
    ) -> PgResult<()> {
        self.client
            .execute(
                &self.stmt_insert,
                &[
                    id,
                    vm_id,
                    &host_architecture,
                    &kernel_name,
                    &base_image,
                    &vcpu_count,
                    &mem_size_mib,
                    &fs_size_mib,
                    &ssh_public_key,
                    &ssh_private_key,
                    &serde_json::to_value(process_sleep_snapshot)?,
                    &serde_json::to_value(volume_sleep_snapshot)?,
                    &serde_json::to_value(remote_files)?,
                ],
            )
            .await?;

        Ok(())
    }

    /// Fetches the most recently created live sleep snapshot for a VM.
    /// Warns if more than one live record exists (indicates a bug).
    pub async fn fetch_latest_by_vm_id(&self, vm_id: &Uuid) -> PgResult<RecordSleepSnapshot> {
        let rows = self
            .client
            .query(&self.stmt_fetch_by_vm_id, &[vm_id])
            .await?;

        if rows.len() > 1 {
            warn!(
                %vm_id,
                count = rows.len(),
                "Multiple live sleep snapshots found for VM"
            );
        }

        rows.into_iter()
            .next()
            .ok_or_else(|| {
                crate::Error::UnexpectedValue(format!(
                    "no live sleep snapshot found for vm_id {vm_id}"
                ))
            })
            .and_then(RecordSleepSnapshot::try_from)
    }

    pub async fn soft_delete_by_id(&self, id: &Uuid) -> PgResult<bool> {
        let affected = self
            .client
            .execute(&self.stmt_soft_delete_by_id, &[id])
            .await?;
        Ok(affected > 0)
    }

    /// Returns true if any sleep snapshot references the given Ceph image name.
    pub async fn image_name_exists(&self, image_name: &str) -> PgResult<bool> {
        let row = self
            .client
            .query_one(&self.stmt_image_name_exists, &[&image_name])
            .await?;
        let count: i64 = row.try_get(0)?;
        Ok(count > 0)
    }
}
