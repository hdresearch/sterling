use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_postgres::{Client, Statement};
use uuid::Uuid;

type PgResult<T> = Result<T, crate::Error>;

const TABLE_NAME: &'static str = "chelsea.vm_usage_segments";

/// Represents the start metadata for a VM usage segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordVmUsageSegmentStart {
    pub vm_id: Uuid,
    pub start_timestamp: i64,
    pub start_created_at: i64,
    pub vcpu_count: u32,
    pub ram_mib: u32,
    pub disk_gib: Option<u32>,
    pub start_code: Option<String>,
}

/// Table interface for chelsea.vm_usage_segments
pub struct TableVmUsageSegment {
    client: Arc<Client>,
    stmt_insert_start: Statement,
    stmt_complete_latest_segment: Statement,
}

impl TableVmUsageSegment {
    pub async fn new(client: Arc<Client>) -> PgResult<Self> {
        Ok(Self {
            stmt_insert_start: client
                .prepare(&format!(
                    "INSERT INTO {TABLE_NAME} (
                        vm_id,
                        start_timestamp,
                        start_created_at,
                        vcpu_count,
                        ram_mib,
                        disk_gib,
                        start_code
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7)"
                ))
                .await?,
            stmt_complete_latest_segment: client
                .prepare(&format!(
                    "
                    WITH latest AS (
                        SELECT vm_id, start_timestamp
                        FROM {TABLE_NAME}
                        WHERE vm_id = $1 AND stop_timestamp IS NULL
                        ORDER BY start_timestamp DESC
                        LIMIT 1
                    )
                    UPDATE {TABLE_NAME} AS seg
                    SET stop_timestamp = $2,
                        stop_created_at = $3,
                        stop_code = $4
                    FROM latest
                    WHERE seg.vm_id = latest.vm_id
                      AND seg.start_timestamp = latest.start_timestamp
                    "
                ))
                .await?,
            client,
        })
    }

    /// Insert a new VM usage segment with start metadata.
    pub async fn insert_start(&self, record: &RecordVmUsageSegmentStart) -> PgResult<()> {
        self.client
            .execute(
                &self.stmt_insert_start,
                &[
                    &record.vm_id,
                    &record.start_timestamp,
                    &record.start_created_at,
                    &record.vcpu_count,
                    &record.ram_mib,
                    &record.disk_gib,
                    &record.start_code,
                ],
            )
            .await
            .map(|_| ())?;

        Ok(())
    }

    /// Completes the most recent open segment for the given vm_id.
    /// Returns true if a segment was updated.
    pub async fn complete_latest_segment(
        &self,
        vm_id: &Uuid,
        stop_timestamp: i64,
        stop_created_at: i64,
        stop_code: Option<&str>,
    ) -> PgResult<bool> {
        let affected = self
            .client
            .execute(
                &self.stmt_complete_latest_segment,
                &[vm_id, &stop_timestamp, &stop_created_at, &stop_code],
            )
            .await?;

        Ok(affected > 0)
    }
}
