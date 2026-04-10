use std::{path::Path, sync::Arc};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_postgres::{Client, Row, Statement};
use uuid::Uuid;

use crate::schema::generic::generic_stmt_fetch_by_id;

const ID_COL_NAME: &'static str = "id";
const TABLE_NAME: &'static str = "chelsea.commit";

type PgResult<T> = Result<T, crate::Error>;

/// chelsea.commit table
pub struct TableCommit {
    client: Arc<Client>,
    stmt_fetch_by_id: Statement,
    stmt_insert: Statement,
    stmt_mark_deleted: Statement,
    stmt_snap_name_exists: Statement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// chelsea.commit record
pub struct RecordCommit {
    pub id: Uuid,
    pub host_architecture: String,
    pub kernel_name: String,
    pub base_image: String,
    pub vcpu_count: u32,
    pub mem_size_mib: u32,
    pub fs_size_mib: u32,
    pub ssh_public_key: String,
    pub ssh_private_key: String,
    pub process_commit: RecordProcessCommit,
    pub volume_commit: RecordVolumeCommit,
    pub remote_files: Vec<CommitFile>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub deleted_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordProcessCommit {
    Firecracker(RecordFirecrackerProcessCommit),
    CloudHypervisor(RecordCloudHypervisorProcessCommit),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordFirecrackerProcessCommit {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordCloudHypervisorProcessCommit {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordVolumeCommit {
    Ceph(RecordCephVolumeCommit),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordCephVolumeCommit {
    pub snap_name: String,
}

impl TryFrom<Row> for RecordCommit {
    type Error = crate::Error;
    fn try_from(value: Row) -> Result<Self, Self::Error> {
        Ok(RecordCommit {
            id: value.try_get("id")?,
            host_architecture: value.try_get("host_architecture")?,
            kernel_name: value.try_get("kernel_name")?,
            base_image: value.try_get("base_image")?,
            vcpu_count: value.try_get("vcpu_count")?,
            mem_size_mib: value.try_get("mem_size_mib")?,
            fs_size_mib: value.try_get("fs_size_mib")?,
            ssh_public_key: value.try_get("ssh_public_key")?,
            ssh_private_key: value.try_get("ssh_private_key")?,
            process_commit: serde_json::from_value(value.try_get::<_, Value>("process_commit")?)?,
            volume_commit: serde_json::from_value(value.try_get::<_, Value>("volume_commit")?)?,
            remote_files: serde_json::from_value(value.try_get("remote_files")?)?,
            deleted_at: value.try_get("deleted_at")?,
            deleted_by: value.try_get("deleted_by")?,
        })
    }
}

/// Information about a file uploaded to the VmCommitStore
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommitFile {
    /// The file's key on the VmCommitStore. Should include prefixes.
    pub key: String,
    // /// The file checksum.
    // TODO: Uncomment and implement
    // pub checksum: String,
}

impl CommitFile {
    /// Compute the filename from the file key, stripping its prefix
    pub fn file_name<'a>(&'a self) -> Result<&'a str, String> {
        Path::new(&self.key)
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .ok_or(format!("Failed to extract file name from key {}", self.key))
    }
}

impl ToString for CommitFile {
    fn to_string(&self) -> String {
        self.key.clone()
    }
}

impl From<String> for CommitFile {
    fn from(value: String) -> Self {
        Self { key: value }
    }
}

impl TableCommit {
    pub async fn new(client: Arc<Client>) -> PgResult<Self> {
        Ok(Self {
            stmt_fetch_by_id: generic_stmt_fetch_by_id(&client, ID_COL_NAME, TABLE_NAME).await?,
            stmt_insert: client
                .prepare(
                    "INSERT INTO chelsea.commit (
                        id, host_architecture, kernel_name, base_image, vcpu_count,
                        mem_size_mib, fs_size_mib, ssh_public_key, ssh_private_key,
                        process_commit, volume_commit, remote_files
                    ) VALUES (
                        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12
                    )",
                )
                .await?,
            stmt_mark_deleted: client
                .prepare(
                    "UPDATE chelsea.commit SET deleted_at = NOW(), deleted_by = $2 \
                     WHERE id = $1 AND deleted_at IS NULL",
                )
                .await?,
            stmt_snap_name_exists: client
                .prepare(
                    "SELECT COUNT(*) FROM chelsea.commit \
                     WHERE volume_commit->'Ceph'->>'snap_name' = $1 \
                     AND deleted_at IS NULL",
                )
                .await?,
            client,
        })
    }

    pub async fn insert(&self, record: &RecordCommit) -> PgResult<()> {
        self.client
            .execute(
                &self.stmt_insert,
                &[
                    &record.id,
                    &record.host_architecture,
                    &record.kernel_name,
                    &record.base_image,
                    &record.vcpu_count,
                    &record.mem_size_mib,
                    &record.fs_size_mib,
                    &record.ssh_public_key,
                    &record.ssh_private_key,
                    &serde_json::to_value(&record.process_commit)?,
                    &serde_json::to_value(&record.volume_commit)?,
                    &serde_json::to_value(&record.remote_files)?,
                ],
            )
            .await?;

        Ok(())
    }

    pub async fn fetch_by_id(&self, id: &Uuid) -> PgResult<RecordCommit> {
        let row = self.client.query_one(&self.stmt_fetch_by_id, &[id]).await?;
        RecordCommit::try_from(row)
    }

    pub async fn get_by_id_opt(&self, id: &Uuid) -> PgResult<Option<RecordCommit>> {
        let row = self.client.query_opt(&self.stmt_fetch_by_id, &[id]).await?;
        match row {
            Some(row) => Ok(Some(RecordCommit::try_from(row)?)),
            None => Ok(None),
        }
    }

    pub async fn mark_deleted(&self, id: &Uuid, deleted_by: Option<Uuid>) -> PgResult<bool> {
        let affected = self
            .client
            .execute(&self.stmt_mark_deleted, &[id, &deleted_by])
            .await?;
        Ok(affected > 0)
    }

    /// Returns true if any non-deleted commit references the given Ceph snap name.
    ///
    /// The snap name is in `"image_name@snap_name"` format as stored in the DB.
    pub async fn snap_name_exists(&self, snap_name: &str) -> PgResult<bool> {
        let row = self
            .client
            .query_one(&self.stmt_snap_name_exists, &[&snap_name])
            .await?;
        let count: i64 = row.try_get(0)?;
        Ok(count > 0)
    }
}
