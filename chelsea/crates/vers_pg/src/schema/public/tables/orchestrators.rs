use std::{
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Client, Row, Statement};
use uuid::Uuid;

use crate::schema::generic::{generic_stmt_delete_by_id, generic_stmt_fetch_by_id};

const ID_COL_NAME: &'static str = "id";
const TABLE_NAME: &'static str = "public.orchestrators";

type PgResult<T> = Result<T, crate::Error>;

/// public.orchestrators table
pub struct TableOrchestrators {
    client: Arc<Client>,
    stmt_fetch_by_id: Statement,
    stmt_fetch_by_region: Statement,
    stmt_insert: Statement,
    stmt_delete_by_id: Statement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// public.orchestrators record
pub struct RecordOrchestrator {
    pub id: Uuid,
    pub region: String,
    pub wg_public_key: String,
    pub wg_private_key: String,
    pub wg_ipv6: Ipv6Addr,
    pub ip: IpAddr,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<Row> for RecordOrchestrator {
    type Error = crate::Error;
    fn try_from(row: Row) -> Result<Self, Self::Error> {
        let wg_ipv6 = match row.try_get::<_, IpAddr>("wg_ipv6")? {
            IpAddr::V6(addr) => addr,
            IpAddr::V4(_) => {
                return Err(crate::Error::UnexpectedValue("wg_ipv6 is not IPv6".into()));
            }
        };
        Ok(Self {
            id: row.try_get("id")?,
            region: row.try_get("region")?,
            wg_public_key: row.try_get("wg_public_key")?,
            wg_private_key: row.try_get("wg_private_key")?,
            wg_ipv6,
            ip: row.try_get("ip")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl TableOrchestrators {
    pub async fn new(client: Arc<Client>) -> PgResult<Self> {
        Ok(Self {
            stmt_fetch_by_id: generic_stmt_fetch_by_id(&client, ID_COL_NAME, TABLE_NAME).await?,
            stmt_fetch_by_region: client
                .prepare("SELECT * FROM public.orchestrators WHERE region = $1")
                .await?,
            stmt_insert: client
                .prepare(
                    "INSERT INTO public.orchestrators \
                        (id, region, wg_public_key, wg_private_key, wg_ipv6, ip) \
                    VALUES \
                        ($1, $2, $3, $4, $5, $6) \
                    RETURNING *",
                )
                .await?,
            stmt_delete_by_id: generic_stmt_delete_by_id(&client, ID_COL_NAME, TABLE_NAME).await?,
            client,
        })
    }

    /// Inserts a new record in the orchestrators table and returns the resulting `RecordOrchestrator`.
    pub async fn insert(
        &self,
        id: &Uuid,
        region: &str,
        wg_public_key: &str,
        wg_private_key: &str,
        wg_ipv6: &Ipv6Addr,
        ip: &IpAddr,
    ) -> PgResult<RecordOrchestrator> {
        let wg_ipv6_addr = IpAddr::V6(*wg_ipv6);
        let row = self
            .client
            .query_one(
                &self.stmt_insert,
                &[
                    id,
                    &region,
                    &wg_public_key,
                    &wg_private_key,
                    &wg_ipv6_addr,
                    ip,
                ],
            )
            .await?;
        Ok(RecordOrchestrator::try_from(row)?)
    }

    pub async fn fetch_by_id(&self, id: &Uuid) -> PgResult<Option<RecordOrchestrator>> {
        let row = self.client.query_opt(&self.stmt_fetch_by_id, &[id]).await?;
        match row {
            Some(row) => Ok(Some(RecordOrchestrator::try_from(row)?)),
            None => Ok(None),
        }
    }

    pub async fn fetch_by_region(&self, region: &str) -> PgResult<Option<RecordOrchestrator>> {
        let row = self
            .client
            .query_opt(&self.stmt_fetch_by_region, &[&region])
            .await?;
        match row {
            Some(row) => Ok(Some(RecordOrchestrator::try_from(row)?)),
            None => Ok(None),
        }
    }

    pub async fn delete_by_id(&self, id: &Uuid) -> PgResult<bool> {
        let affected = self.client.execute(&self.stmt_delete_by_id, &[id]).await?;
        Ok(affected > 0)
    }
}
