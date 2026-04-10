use std::{collections::HashSet, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_postgres::{Client, Row, Statement};
use uuid::Uuid;

use crate::schema::generic::{generic_stmt_delete_by_id, generic_stmt_fetch_by_id};

const ID_COL_NAME: &'static str = "id";
const TABLE_NAME: &'static str = "chelsea.vm";

type PgResult<T> = Result<T, crate::Error>;

/// chelsea.vm table
pub struct TableVm {
    client: Arc<Client>,
    stmt_fetch_by_id: Statement,
    stmt_insert: Statement,
    stmt_delete_by_id: Statement,
    stmt_image_name_exists: Statement,
    stmt_fetch_all_volumes: Statement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// chelsea.vm record
pub struct RecordVm {
    pub id: Uuid,
    pub volume: RecordVmVolume,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordVmVolume {
    Ceph(RecordCephVmVolume),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordCephVmVolume {
    pub image_name: String,
}

impl TryFrom<Row> for RecordVm {
    type Error = crate::Error;
    fn try_from(value: Row) -> Result<Self, Self::Error> {
        Ok(RecordVm {
            id: value.try_get("id")?,
            volume: serde_json::from_value(value.try_get("volume")?)?,
        })
    }
}

impl TableVm {
    pub async fn new(client: Arc<Client>) -> PgResult<Self> {
        Ok(Self {
            stmt_fetch_by_id: generic_stmt_fetch_by_id(&client, ID_COL_NAME, TABLE_NAME).await?,
            stmt_insert: client
                .prepare("INSERT INTO chelsea.vm (id, volume) VALUES ($1, $2)")
                .await?,
            stmt_delete_by_id: generic_stmt_delete_by_id(&client, ID_COL_NAME, TABLE_NAME).await?,
            stmt_image_name_exists: client
                .prepare(
                    "SELECT COUNT(*) FROM chelsea.vm \
                     WHERE volume->'ceph'->>'image_name' = $1",
                )
                .await?,
            stmt_fetch_all_volumes: client.prepare("SELECT volume FROM chelsea.vm").await?,
            client,
        })
    }

    pub async fn insert(&self, record: &RecordVm) -> PgResult<()> {
        self.client
            .execute(
                &self.stmt_insert,
                &[&record.id, &serde_json::to_value(&record.volume)?],
            )
            .await?;

        Ok(())
    }

    pub async fn fetch_by_id(&self, id: &Uuid) -> PgResult<RecordVm> {
        let row = self.client.query_one(&self.stmt_fetch_by_id, &[id]).await?;
        RecordVm::try_from(row)
    }

    pub async fn delete_by_id(&self, id: &Uuid) -> PgResult<bool> {
        let affected = self.client.execute(&self.stmt_delete_by_id, &[id]).await?;
        Ok(affected > 0)
    }

    /// Returns true if any VM references the given Ceph image name.
    pub async fn image_name_exists(&self, image_name: &str) -> PgResult<bool> {
        let row = self
            .client
            .query_one(&self.stmt_image_name_exists, &[&image_name])
            .await?;
        let count: i64 = row.try_get(0)?;
        Ok(count > 0)
    }

    /// Returns the set of all Ceph image names referenced by active VMs.
    pub async fn fetch_all_active_image_names(&self) -> PgResult<HashSet<String>> {
        let rows = self.client.query(&self.stmt_fetch_all_volumes, &[]).await?;
        let mut names = HashSet::with_capacity(rows.len());
        for row in rows {
            let volume: RecordVmVolume =
                serde_json::from_value(row.try_get::<_, Value>("volume")?)?;
            match volume {
                RecordVmVolume::Ceph(c) => {
                    names.insert(c.image_name);
                }
            }
        }
        Ok(names)
    }
}
