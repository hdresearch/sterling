use std::{
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Client, Row, Statement};
use uuid::Uuid;

use crate::{
    Error,
    schema::generic::{generic_stmt_delete_by_id, generic_stmt_fetch_by_id},
};

const ID_COL_NAME: &'static str = "node_id";
const TABLE_NAME: &'static str = "public.nodes";

type PgResult<T> = Result<T, crate::Error>;

/// public.nodes table
pub struct TableNodes {
    client: Arc<Client>,
    stmt_fetch_by_id: Statement,
    stmt_insert: Statement,
    stmt_delete_by_id: Statement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// public.nodes record
pub struct RecordNode {
    pub node_id: Uuid,
    pub ip: IpAddr,
    pub created_at: DateTime<Utc>,
    pub under_orchestrator_id: Uuid,
    pub wg_ipv6: Ipv6Addr,
    pub wg_public_key: String,
    pub wg_private_key: String,
    pub cpu_cores_total: i32,
    pub memory_mib_total: i64,
    pub disk_size_mib_total: i64,
    pub network_count_total: i32,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<Row> for RecordNode {
    type Error = crate::Error;
    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(Self {
            node_id: row.try_get("node_id")?,
            ip: row.try_get("ip")?,
            created_at: row.try_get("created_at")?,
            under_orchestrator_id: row.try_get("under_orchestrator_id")?,
            wg_ipv6: match row.try_get::<_, IpAddr>("wg_ipv6")? {
                IpAddr::V4(addr) => {
                    return Err(Error::UnexpectedValue(format!(
                        "Expected IPv6 address for node.wg_ipv6, got {addr}"
                    )));
                }
                IpAddr::V6(addr) => addr,
            },
            wg_public_key: row.try_get("wg_public_key")?,
            wg_private_key: row.try_get("wg_private_key")?,
            cpu_cores_total: row.try_get("cpu_cores_total")?,
            memory_mib_total: row.try_get("memory_mib_total")?,
            disk_size_mib_total: row.try_get("disk_size_mib_total")?,
            network_count_total: row.try_get("network_count_total")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl TableNodes {
    pub async fn new(client: Arc<Client>) -> PgResult<Self> {
        Ok(Self {
            stmt_fetch_by_id: generic_stmt_fetch_by_id(&client, ID_COL_NAME, TABLE_NAME).await?,
            stmt_insert: client
                .prepare(
                    "INSERT INTO public.nodes \
                        (node_id, ip, under_orchestrator_id, \
                        wg_public_key, wg_private_key, cpu_cores_total, \
                        memory_mib_total, disk_size_mib_total, network_count_total) \
                    VALUES \
                        ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                    RETURNING *",
                )
                .await?,
            stmt_delete_by_id: generic_stmt_delete_by_id(&client, ID_COL_NAME, TABLE_NAME).await?,
            client,
        })
    }

    /// Inserts a new record in the nodes table and returns the resulting `RecordNode`. WG IPv6 is assigned by trigger setup_ipv6_assigns_on_chelsea_insert.
    pub async fn insert(
        &self,
        node_id: &Uuid,
        ip: &IpAddr,
        under_orchestrator_id: &Uuid,
        wg_public_key: &str,
        wg_private_key: &str,
        cpu_cores_total: i32,
        memory_mib_total: i64,
        disk_size_mib_total: i64,
        network_count_total: i32,
    ) -> PgResult<RecordNode> {
        let row = self
            .client
            .query_one(
                &self.stmt_insert,
                &[
                    node_id,
                    &ip,
                    under_orchestrator_id,
                    &wg_public_key,
                    &wg_private_key,
                    &cpu_cores_total,
                    &memory_mib_total,
                    &disk_size_mib_total,
                    &network_count_total,
                ],
            )
            .await?;
        Ok(RecordNode::try_from(row)?)
    }

    pub async fn fetch_by_id(&self, node_id: &Uuid) -> PgResult<Option<RecordNode>> {
        let row = self
            .client
            .query_opt(&self.stmt_fetch_by_id, &[node_id])
            .await?;
        match row {
            Some(row) => Ok(Some(RecordNode::try_from(row)?)),
            None => Ok(None),
        }
    }

    pub async fn delete_by_id(&self, node_id: &Uuid) -> PgResult<bool> {
        let affected = self
            .client
            .execute(&self.stmt_delete_by_id, &[node_id])
            .await?;
        Ok(affected > 0)
    }
}
