use std::net::{IpAddr, Ipv6Addr, SocketAddrV6};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use utoipa::ToSchema;
use uuid::Uuid;
use vers_config::VersConfig;

use crate::db::{DB, DBError};

pub trait ChelseaNodeRepository {
    /// If wg_ipv6.is_some() it overrides the autoassignment in pg.
    fn insert(
        &self,
        node_id: Uuid,
        under_orchestrator_id: &Uuid,
        resources: &NodeResources,
        wg_private_key: &str,
        wg_public_key: &str,
        wg_ipv6: Option<Ipv6Addr>,
        ip: Option<IpAddr>,
    ) -> impl Future<Output = Result<NodeEntity, DBError>>;

    fn all_under_orchestrator(
        &self,
        orchestrator_id: &Uuid,
    ) -> impl Future<Output = Result<Vec<NodeEntity>, DBError>>;

    fn set_node_instance(
        &self,
        node_id: Uuid,
        ip: IpAddr,
    ) -> impl Future<Output = Result<(), DBError>>;

    fn delete(&self, node_id: &Uuid) -> impl Future<Output = Result<(), DBError>>;

    fn get_by_id(
        &self,
        node_id: &Uuid,
    ) -> impl Future<Output = Result<Option<NodeEntity>, DBError>>;

    /// Update the hardware resource totals for a node.
    /// Called when telemetry reports the actual hardware capacity.
    fn update_resources(
        &self,
        node_id: &Uuid,
        cpu_total: i32,
        memory_mib_total: i64,
    ) -> impl Future<Output = Result<(), DBError>>;
}

#[allow(unused)] // It's a pg table entity.
#[derive(Debug)]
pub struct NodeEntity {
    id: Uuid,
    under_orchestrator_id: Uuid,
    ip: IpAddr,
    wg_private_key: String,
    wg_public_key: String,
    wg_ipv6: Ipv6Addr,
    resources: NodeResources,

    created_at: DateTime<Utc>,
}

impl DB {
    pub fn node(&self) -> Node {
        Node(self.clone())
    }
}

impl NodeEntity {
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    pub fn ip_pub(&self) -> IpAddr {
        self.ip
    }

    pub fn ip_priv(&self) -> Ipv6Addr {
        self.wg_ipv6
    }

    pub fn set_ip_pub(&mut self, ip: IpAddr) {
        self.ip = ip;
    }

    pub fn wg_pub_key(&self) -> &str {
        self.wg_public_key.as_str()
    }

    // Should this go on the node record?
    pub fn server_port(&self) -> u16 {
        VersConfig::chelsea().server_port
    }

    /// Returns the address to which the orchestrator should send packets destined for the node's REST API.
    pub fn server_addr(&self) -> SocketAddrV6 {
        SocketAddrV6::new(self.ip_priv(), self.server_port(), 0, 0)
    }

    /// Get the node's hardware resources
    pub fn resources(&self) -> &NodeResources {
        &self.resources
    }
}

#[cfg(test)]
impl NodeEntity {
    /// Create a minimal NodeEntity for unit testing.
    ///
    /// Only `id` and `resources` are meaningful for the node selection algorithm;
    /// other fields are filled with safe defaults.
    pub fn for_test(id: Uuid, resources: NodeResources) -> Self {
        Self {
            id,
            under_orchestrator_id: Uuid::nil(),
            ip: IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            wg_private_key: String::new(),
            wg_public_key: String::new(),
            wg_ipv6: Ipv6Addr::LOCALHOST,
            resources,
            created_at: Utc::now(),
        }
    }
}

impl From<Row> for NodeEntity {
    fn from(row: Row) -> Self {
        let resources = NodeResources::new(
            row.get("cpu_cores_total"),
            row.get("memory_mib_total"),
            row.get("disk_size_mib_total"),
            row.get("network_count_total"),
        );

        let wg_public_key = row.get("wg_public_key");
        let wg_private_key = row.get("wg_private_key");

        let wg_ipv6 = match row.get("wg_ipv6") {
            IpAddr::V6(ipv6) => ipv6,
            IpAddr::V4(_) => unreachable!("for now"),
        };
        let under_orchestrator_id = row.get("under_orchestrator_id");

        // TODO: switch to INET.
        let ip = row.get::<_, IpAddr>("ip");

        NodeEntity {
            id: row.get("node_id"),
            under_orchestrator_id,
            ip,
            wg_ipv6,
            wg_public_key,
            wg_private_key,
            resources,

            created_at: row.get("created_at"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct NodeResources {
    /// total cores machine has.
    hardware_cpu: i32,
    /// total memory machine has.
    hardware_memory_mib: i64,
    /// total disk machine has.
    hardware_disk_mib: i64,
    /// Networks to hand to VMs
    network_count_total: i32,
}

impl NodeResources {
    /// Create a new NodeResources instance.
    ///
    /// # Note on zero values
    ///
    /// Zero values are valid for newly registered nodes that haven't reported
    /// telemetry yet. The node selection algorithm handles this by:
    /// 1. Returning score 0 for nodes with zero resources (they won't be preferred)
    /// 2. Filtering out such nodes when VM requirements are specified
    ///
    /// Once the node reports telemetry via health checks, `update_resources`
    /// updates the hardware totals in the DB to match the real values.
    pub fn new(cpu: i32, memory_mib: i64, disk_mib: i64, network_count: i32) -> Self {
        Self {
            hardware_cpu: cpu,
            hardware_memory_mib: memory_mib,
            hardware_disk_mib: disk_mib,
            network_count_total: network_count,
        }
    }

    /// Get total CPU cores on this node
    pub fn hardware_cpu(&self) -> i32 {
        self.hardware_cpu
    }

    /// Get total memory in MiB on this node
    pub fn hardware_memory_mib(&self) -> i64 {
        self.hardware_memory_mib
    }
}

pub struct Node(DB);

impl ChelseaNodeRepository for Node {
    async fn set_node_instance(&self, node_id: Uuid, ip: IpAddr) -> Result<(), DBError> {
        let option_row = execute_sql!(
            self.0,
            "UPDATE nodes SET ip = $1 WHERE node_id = $2",
            &[Type::INET, Type::UUID],
            &[&ip, &node_id]
        )?;

        debug_assert!(option_row <= 1);

        Ok(())
    }
    async fn get_by_id(&self, node_id: &Uuid) -> Result<Option<NodeEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM nodes WHERE node_id = $1",
            &[Type::INET],
            &[&node_id]
        )?;

        Ok(row.map(|row| row.into()))
    }
    async fn insert(
        &self,
        node_id: Uuid,
        under_orchestrator_id: &Uuid,
        resources: &NodeResources,
        wg_private_key: &str,
        wg_public_key: &str,
        wg_ipv6: Option<Ipv6Addr>,
        ip: Option<IpAddr>,
    ) -> Result<NodeEntity, DBError> {
        let created_at = Utc::now();

        // wg_ipv6 is not included here as we have a function that genereates that for us and
        let option_row = query_one_sql!(
            self.0,
            "INSERT INTO nodes (
                node_id, under_orchestrator_id, wg_private_key, wg_public_key, wg_ipv6,

                ip, 

                cpu_cores_total, memory_mib_total, disk_size_mib_total, network_count_total,

                created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING *",
            &[
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::TEXT,
                Type::INET,
                Type::INET,
                Type::INT4,
                Type::INT8,
                Type::INT8,
                Type::INT4,
                Type::TIMESTAMPTZ,
            ],
            &[
                &node_id,
                &under_orchestrator_id,
                &wg_private_key,
                &wg_public_key,
                &wg_ipv6.map(IpAddr::from),
                &ip,
                &resources.hardware_cpu,
                &resources.hardware_memory_mib,
                &resources.hardware_disk_mib,
                &resources.network_count_total,
                &created_at,
            ]
        )?;

        Ok(NodeEntity::from(option_row.unwrap()))
    }

    async fn delete(&self, node_id: &Uuid) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "DELETE FROM nodes WHERE node_id = $1",
            &[Type::UUID],
            &[&node_id]
        )?;

        debug_assert!(rows >= 1);

        Ok(())
    }

    async fn all_under_orchestrator(
        &self,
        orchestrator_id: &Uuid,
    ) -> Result<Vec<NodeEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM nodes WHERE under_orchestrator_id = $1",
            &[Type::UUID],
            &[&orchestrator_id]
        )?;

        let vec = rows.into_iter().map(|row| row.into()).collect();

        Ok(vec)
    }

    async fn update_resources(
        &self,
        node_id: &Uuid,
        cpu_total: i32,
        memory_mib_total: i64,
    ) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "UPDATE nodes SET cpu_cores_total = $1, memory_mib_total = $2 WHERE node_id = $3",
            &[Type::INT4, Type::INT8, Type::UUID],
            &[&cpu_total, &memory_mib_total, &node_id]
        )?;

        Ok(())
    }
}
