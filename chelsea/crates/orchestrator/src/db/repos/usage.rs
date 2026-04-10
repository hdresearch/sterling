use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

#[derive(Clone)]
pub struct Usage(DB);

impl DB {
    pub fn usage(&self) -> Usage {
        Usage(self.clone())
    }
}

#[derive(Debug, Clone)]
pub struct UsageVmDescriptor {
    pub vm_id: Uuid,
    pub owner_id: Uuid,
    /// `None` while the VM is sleeping.
    pub node_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub enum VmEvent {
    Start(VmStartEvent),
    Stop(VmStopEvent),
}

impl VmEvent {
    pub fn timestamp(&self) -> i64 {
        match self {
            VmEvent::Start(event) => event.timestamp,
            VmEvent::Stop(event) => event.timestamp,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VmStartEvent {
    pub timestamp: i64,
    pub vcpu_count: u32,
    pub ram_mib: u32,
}

#[derive(Debug, Clone)]
pub struct VmStopEvent {
    pub timestamp: i64,
}

impl Usage {
    pub async fn get_last_reported_interval(
        &self,
        orchestrator_id: &Uuid,
    ) -> Result<Option<(i64, i64)>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT last_interval_start, last_interval_end
             FROM usage_reporting_state
             WHERE orchestrator_id = $1",
            &[Type::UUID],
            &[orchestrator_id]
        )?;

        Ok(maybe_row.map(|row| {
            let start = row.get::<_, i64>("last_interval_start");
            let end = row.get::<_, i64>("last_interval_end");
            (start, end)
        }))
    }

    pub async fn update_last_reported_interval(
        &self,
        orchestrator_id: &Uuid,
        interval_start: i64,
        interval_end: i64,
    ) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "INSERT INTO usage_reporting_state (orchestrator_id, last_interval_start, last_interval_end, last_report_time)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (orchestrator_id) DO UPDATE
             SET last_interval_start = EXCLUDED.last_interval_start,
                 last_interval_end = EXCLUDED.last_interval_end,
                 last_report_time = EXCLUDED.last_report_time",
            &[Type::UUID, Type::INT8, Type::INT8],
            &[orchestrator_id, &interval_start, &interval_end]
        )?;
        Ok(())
    }

    pub async fn get_vms_with_usage_in_interval(
        &self,
        interval_start: i64,
        interval_end: i64,
    ) -> Result<Vec<UsageVmDescriptor>, DBError> {
        let rows = query_sql!(
            self.0,
            "
            SELECT v.vm_id, v.owner_id, v.node_id
            FROM vms v
            WHERE EXISTS (
                SELECT 1
                FROM chelsea.vm_usage_segments seg
                WHERE seg.vm_id = v.vm_id
                  AND seg.start_timestamp < $2
                  AND (seg.stop_timestamp IS NULL OR seg.stop_timestamp > $1)
            )
            ",
            &[Type::INT8, Type::INT8],
            &[&interval_start, &interval_end]
        )?;

        Ok(rows
            .into_iter()
            .map(|row| UsageVmDescriptor {
                vm_id: row.get("vm_id"),
                owner_id: row.get("owner_id"),
                node_id: row.get("node_id"),
            })
            .collect())
    }

    pub async fn get_last_vm_event_before(
        &self,
        vm_id: &Uuid,
        before_timestamp: i64,
    ) -> Result<Option<VmEvent>, DBError> {
        let rows = query_sql!(
            self.0,
            "
            SELECT event_type, timestamp, vcpu_count, ram_mib
            FROM (
                SELECT 'start' AS event_type, s.timestamp, s.vcpu_count, s.ram_mib
                FROM (
                    SELECT start_timestamp AS timestamp, vcpu_count::INT4 AS vcpu_count, ram_mib::INT4 AS ram_mib
                    FROM chelsea.vm_usage_segments
                    WHERE vm_id = $1 AND start_timestamp < $2
                    ORDER BY start_timestamp DESC
                    LIMIT 1
                ) s
                UNION ALL
                SELECT 'stop' AS event_type, t.timestamp, NULL::INT4, NULL::INT4
                FROM (
                    SELECT stop_timestamp AS timestamp
                    FROM chelsea.vm_usage_segments
                    WHERE vm_id = $1 AND stop_timestamp IS NOT NULL AND stop_timestamp < $2
                    ORDER BY stop_timestamp DESC
                    LIMIT 1
                ) t
            ) events
            ORDER BY timestamp DESC
            LIMIT 1
            ",
            &[Type::UUID, Type::INT8],
            &[vm_id, &before_timestamp]
        )?;

        Ok(rows
            .into_iter()
            .next()
            .map(|row| row_into_event(row).expect("invalid event row")))
    }

    pub async fn get_vm_events(
        &self,
        vm_id: &Uuid,
        interval_start: i64,
        interval_end: i64,
    ) -> Result<Vec<VmEvent>, DBError> {
        let rows = query_sql!(
            self.0,
            "
            SELECT event_type, timestamp, vcpu_count, ram_mib
            FROM (
                SELECT 'start' AS event_type, start_timestamp AS timestamp, vcpu_count::INT4 AS vcpu_count, ram_mib::INT4 AS ram_mib
                FROM chelsea.vm_usage_segments
                WHERE vm_id = $1 AND start_timestamp >= $2 AND start_timestamp < $3
                UNION ALL
                SELECT 'stop' AS event_type, stop_timestamp AS timestamp, NULL::INT4, NULL::INT4
                FROM chelsea.vm_usage_segments
                WHERE vm_id = $1 AND stop_timestamp IS NOT NULL AND stop_timestamp >= $2 AND stop_timestamp < $3
            ) events
            ORDER BY timestamp
            ",
            &[Type::UUID, Type::INT8, Type::INT8],
            &[vm_id, &interval_start, &interval_end]
        )?;

        Ok(rows
            .into_iter()
            .map(|row| row_into_event(row).expect("invalid event row"))
            .collect())
    }
}

fn row_into_event(row: Row) -> Result<VmEvent, &'static str> {
    let event_type: String = row.get("event_type");
    let timestamp: i64 = row.get("timestamp");

    match event_type.as_str() {
        "start" => {
            let vcpu_count: Option<i32> = row.get("vcpu_count");
            let ram_mib: Option<i32> = row.get("ram_mib");
            let vcpu = vcpu_count.ok_or("missing vcpu_count for start event")?;
            let ram = ram_mib.ok_or("missing ram_mib for start event")?;

            let vcpu_u32 = u32::try_from(vcpu).map_err(|_| "invalid vcpu_count")?;
            let ram_u32 = u32::try_from(ram).map_err(|_| "invalid ram_mib")?;

            Ok(VmEvent::Start(VmStartEvent {
                timestamp,
                vcpu_count: vcpu_u32,
                ram_mib: ram_u32,
            }))
        }
        "stop" => Ok(VmEvent::Stop(VmStopEvent { timestamp })),
        _ => Err("unknown event type"),
    }
}
