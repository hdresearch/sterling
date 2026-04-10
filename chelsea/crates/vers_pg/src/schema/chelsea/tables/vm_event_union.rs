use std::sync::Arc;

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use tokio_postgres::types::private::BytesMut;
use tokio_postgres::types::{FromSql, IsNull, ToSql, Type, accepts, to_sql_checked};
use tokio_postgres::{Client, Row, Statement};
use uuid::Uuid;

type PgResult<T> = Result<T, crate::Error>;

const TABLE_NAME: &'static str = "chelsea.vm_usage_segments";

/// Represents a row emitted when treating `chelsea.vm_usage_segments` start/stop timestamps as separate events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordVmEventUnion {
    Start(RecordVmEventUnionStart),
    Stop(RecordVmEventUnionStop),
}

impl RecordVmEventUnion {
    pub fn timestamp(&self) -> i64 {
        match self {
            Self::Start(event) => event.timestamp,
            Self::Stop(event) => event.timestamp,
        }
    }
}

impl TryFrom<Row> for RecordVmEventUnion {
    type Error = crate::Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        let event_type: EventType = row.try_get("event_type")?;
        match event_type {
            EventType::Start => Ok(RecordVmEventUnion::Start(
                RecordVmEventUnionStart::try_from(row)?,
            )),
            EventType::Stop => Ok(RecordVmEventUnion::Stop(RecordVmEventUnionStop::try_from(
                row,
            )?)),
        }
    }
}

/// The Start variant of RecordVmEventUnion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordVmEventUnionStart {
    pub timestamp: i64,
    pub ram_mib: u32,
    pub vcpu_count: u32,
}

impl TryFrom<Row> for RecordVmEventUnionStart {
    type Error = crate::Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(RecordVmEventUnionStart {
            timestamp: row.try_get("timestamp")?,
            ram_mib: row.try_get("ram_mib")?,
            vcpu_count: row.try_get("vcpu_count")?,
        })
    }
}

/// The Stop variant of RecordVmEventUnion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordVmEventUnionStop {
    pub timestamp: i64,
}

impl TryFrom<Row> for RecordVmEventUnionStop {
    type Error = crate::Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(RecordVmEventUnionStop {
            timestamp: row.try_get("timestamp")?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Start,
    Stop,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EventType::Start => "start",
            EventType::Stop => "stop",
        };
        write!(f, "{}", s)
    }
}

impl<'a> FromSql<'a> for EventType {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<EventType, Box<dyn Error + Sync + Send>> {
        let s = <&str as FromSql>::from_sql(ty, raw)?;
        match s {
            "start" => Ok(EventType::Start),
            "stop" => Ok(EventType::Stop),
            other => Err(format!("Unknown event_type: {}", other).into()),
        }
    }

    accepts!(TEXT, VARCHAR);
}

impl ToSql for EventType {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let s = match self {
            EventType::Start => "start",
            EventType::Stop => "stop",
        };
        <&str as ToSql>::to_sql(&s, ty, out)
    }

    accepts!(TEXT, VARCHAR);
    to_sql_checked!();
}

/// Table interface for fetching the last VM event (start or stop) before a given timestamp.
pub struct UnionVmEvent {
    client: Arc<Client>,
    stmt_get_last_vm_event_before: Statement,
    stmt_get_vms_with_events_in_interval: Statement,
}

impl UnionVmEvent {
    pub async fn new(client: Arc<Client>) -> PgResult<Self> {
        Ok(Self {
            stmt_get_last_vm_event_before: client
                .prepare(&format!(
                    "
                SELECT event_type, timestamp, vcpu_count, ram_mib
                FROM (
                  SELECT 'start' AS event_type, s.timestamp, s.vcpu_count, s.ram_mib
                  FROM (
                    SELECT start_timestamp AS timestamp, vcpu_count::INT4 AS vcpu_count, ram_mib::INT4 AS ram_mib
                    FROM {TABLE_NAME}
                    WHERE vm_id = $1 AND start_timestamp < $2
                    ORDER BY start_timestamp DESC
                    LIMIT 1
                  ) AS s

                  UNION ALL

                  SELECT 'stop' AS event_type, t.timestamp, NULL::INT4, NULL::INT4
                  FROM (
                    SELECT stop_timestamp AS timestamp
                    FROM {TABLE_NAME}
                    WHERE vm_id = $1 AND stop_timestamp IS NOT NULL AND stop_timestamp < $2
                    ORDER BY stop_timestamp DESC
                    LIMIT 1
                  ) AS t
                ) AS events
                ORDER BY timestamp DESC
                LIMIT 1
                "))
                .await?,

            stmt_get_vms_with_events_in_interval: client
                .prepare(&format!(
                    "
                    SELECT DISTINCT vm_id
                    FROM {TABLE_NAME}
                    WHERE start_timestamp < $2
                      AND (stop_timestamp IS NULL OR stop_timestamp > $1)
                    "))
                .await?,
            client,
        })
    }

    /// Fetch the last VM event (start or stop) before a given timestamp.
    /// Returns (event_type, timestamp, Option<vcpu_count>, Option<ram_mib>), where event_type is "start" or "stop".
    pub async fn get_last_vm_event_before(
        &self,
        vm_id: &Uuid,
        before_timestamp: i64,
    ) -> PgResult<Option<RecordVmEventUnion>> {
        let row_opt = self
            .client
            .query_opt(
                &self.stmt_get_last_vm_event_before,
                &[&vm_id, &before_timestamp],
            )
            .await?;

        if let Some(row) = row_opt {
            let event = RecordVmEventUnion::try_from(row)?;
            Ok(Some(event))
        } else {
            Ok(None)
        }
    }

    /// Fetch all vm_ids with start or stop events in a given interval, or still running VMs.
    pub async fn get_vms_with_events_in_interval(
        &self,
        start_time: i64,
        end_time: i64,
    ) -> PgResult<Vec<Uuid>> {
        let rows = self
            .client
            .query(
                &self.stmt_get_vms_with_events_in_interval,
                &[&start_time, &end_time],
            )
            .await?;
        Ok(rows.into_iter().map(|row| row.get::<_, Uuid>(0)).collect())
    }
}
