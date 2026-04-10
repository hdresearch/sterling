use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait HealthCheckRepository {
    fn insert(
        &self,
        node_id: Uuid,
        status: NodeStatus,
        telemetry: Option<HealthCheckTelemetry>,
    ) -> impl Future<Output = Result<HealthCheckEntity, DBError>>;

    fn delete_by_node_id(&self, node_id: &Uuid) -> impl Future<Output = Result<(), DBError>>;

    fn last_5(
        &self,
        node_id: &Uuid,
    ) -> impl Future<Output = Result<Vec<HealthCheckEntity>, DBError>>;
}

pub struct HealthCheck(DB);

impl DB {
    pub fn health(&self) -> HealthCheck {
        HealthCheck(self.clone())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    /// Running as usual.
    Up,
    /// Booting
    Booting,
    /// Not responding to health checks.
    Down,
    /// Migrating all it's VMs to other host. Could be because of downsize.
    Evicting,
    #[serde(other)]
    Unknown,
}

impl From<&'_ str> for NodeStatus {
    fn from(value: &'_ str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "up" => NodeStatus::Up,
            "down" => NodeStatus::Down,
            "evicting" => NodeStatus::Evicting,
            "booting" => NodeStatus::Booting,
            _ => NodeStatus::Unknown,
        }
    }
}

impl NodeStatus {
    pub fn is_up(&self) -> bool {
        self == &Self::Up
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            NodeStatus::Up => "up",
            NodeStatus::Down => "down",
            NodeStatus::Booting => "booting",
            NodeStatus::Evicting => "evicting",
            NodeStatus::Unknown => "unknown",
        }
    }

    pub fn can_change_to(&self, status: NodeStatus) -> bool {
        match self {
            NodeStatus::Up => matches!(status, NodeStatus::Down | NodeStatus::Evicting),
            NodeStatus::Down => matches!(status, NodeStatus::Down | NodeStatus::Evicting),
            NodeStatus::Booting => matches!(status, NodeStatus::Up | NodeStatus::Down),
            NodeStatus::Unknown => true,
            NodeStatus::Evicting => false,
        }
    }
}

/// Telemetry data captured during a health check.
/// These fields are populated when the node is Up and telemetry fetch succeeds.
#[derive(Clone, Debug, Default)]
pub struct HealthCheckTelemetry {
    /// Available vCPUs on the node (total - allocated to VMs)
    pub vcpu_available: Option<i32>,
    /// Available memory in MiB on the node (total - allocated to VMs)
    pub mem_mib_available: Option<i64>,
}

// todo remove clone
#[derive(Clone, Debug)]
pub struct HealthCheckEntity {
    node_id: Uuid,
    timestamp: DateTime<Utc>,
    status: NodeStatus,
    /// Telemetry data from the node at time of health check
    telemetry: HealthCheckTelemetry,
}

impl HealthCheckEntity {
    pub fn new(node_id: Uuid, status: NodeStatus, timestamp: DateTime<Utc>) -> Self {
        Self {
            node_id,
            status,
            timestamp,
            telemetry: HealthCheckTelemetry::default(),
        }
    }

    pub fn with_telemetry(
        node_id: Uuid,
        status: NodeStatus,
        timestamp: DateTime<Utc>,
        telemetry: HealthCheckTelemetry,
    ) -> Self {
        Self {
            node_id,
            status,
            timestamp,
            telemetry,
        }
    }

    pub fn status(&self) -> &NodeStatus {
        &self.status
    }

    pub fn node_id(&self) -> Uuid {
        self.node_id
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    /// Get available vCPUs from cached telemetry
    pub fn vcpu_available(&self) -> Option<i32> {
        self.telemetry.vcpu_available
    }

    /// Get available memory in MiB from cached telemetry
    pub fn mem_mib_available(&self) -> Option<i64> {
        self.telemetry.mem_mib_available
    }
}

impl From<Row> for HealthCheckEntity {
    fn from(row: Row) -> Self {
        Self {
            node_id: row.get("node_id"),
            status: row
                .try_get::<_, String>("status")
                .map(|test| NodeStatus::from(&test as &str))
                .unwrap(),
            timestamp: row.get("timestamp"),
            telemetry: HealthCheckTelemetry {
                vcpu_available: row.try_get("vcpu_available").ok(),
                mem_mib_available: row.try_get("mem_mib_available").ok(),
            },
        }
    }
}

impl HealthCheckRepository for HealthCheck {
    async fn insert(
        &self,
        node_id: Uuid,
        status: NodeStatus,
        telemetry: Option<HealthCheckTelemetry>,
    ) -> Result<HealthCheckEntity, DBError> {
        let telemetry = telemetry.unwrap_or_default();
        let entity = HealthCheckEntity {
            node_id,
            timestamp: Utc::now(),
            status,
            telemetry: telemetry.clone(),
        };
        let rows_affected = execute_sql!(
            self.0,
            "INSERT INTO node_heartbeats (node_id, status, timestamp, vcpu_available, mem_mib_available) VALUES ($1, $2, $3, $4, $5)",
            &[
                Type::UUID,
                Type::TEXT,
                Type::TIMESTAMPTZ,
                Type::INT4,
                Type::INT8
            ],
            &[
                &entity.node_id,
                &entity.status.as_str(),
                &entity.timestamp,
                &telemetry.vcpu_available,
                &telemetry.mem_mib_available
            ]
        )?;

        debug_assert!(rows_affected == 1);

        Ok(entity)
    }

    async fn delete_by_node_id(&self, node_id: &Uuid) -> Result<(), DBError> {
        let rows_affected = execute_sql!(
            self.0,
            "DELETE FROM node_heartbeats WHERE node_id = $1",
            &[Type::UUID],
            &[&node_id]
        )?;

        if rows_affected == 0 {
            tracing::warn!("HealthCheckEntity::delete_by_node_id: 'rows_affected' was 0");
        };

        Ok(())
    }

    async fn last_5(&self, node_id: &Uuid) -> Result<Vec<HealthCheckEntity>, DBError> {
        let result = query_sql!(
            self.0,
            "SELECT * FROM node_heartbeats WHERE node_id = $1 ORDER BY timestamp DESC LIMIT 5",
            &[Type::UUID],
            &[&node_id]
        )?;

        Ok(result.into_iter().map(HealthCheckEntity::from).collect())
    }
}
