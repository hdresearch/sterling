use chrono::{DateTime, Utc};
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv6Addr},
};
use thiserror::Error;
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait VMsRepository {
    fn insert(
        &self,
        vm_id: Uuid,
        parent_commit_id: Option<Uuid>,
        grandparent_vm_id: Option<Uuid>,
        node_id: Uuid,
        ip: Ipv6Addr,
        wg_private_key: String,
        wg_public_key: String,
        wg_port: u16,
        owner_id: Uuid,
        created_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
        vcpu_count: i32,
        mem_size_mib: i32,
        labels: Option<HashMap<String, String>>,
    ) -> impl Future<Output = Result<VmEntity, VMInsertError>>;

    /// List all VMs, except those that have been deleted
    fn list(&self) -> impl Future<Output = Result<Vec<VmEntity>, DBError>>;

    fn get_by_id(&self, vm_id: Uuid) -> impl Future<Output = Result<Option<VmEntity>, DBError>>;

    /// List all VMs under a particular node, except those that have been deleted
    fn list_under_node(
        &self,
        node_id: Uuid,
    ) -> impl Future<Output = Result<Vec<VmEntity>, DBError>>;

    /// List all VMs that are grandchildren of a particular VM, except those that have been deleted
    fn list_grandchild_vms(
        &self,
        vm_id: Uuid,
    ) -> impl Future<Output = Result<Vec<VmEntity>, DBError>>;

    /// Mark a VM as deleted
    fn mark_deleted(&self, vm_id: &Uuid) -> impl Future<Output = Result<(), DBError>>;

    /// List all VMs under a particular org ID, except those that have been deleted
    fn list_by_org_id(&self, org_id: Uuid) -> impl Future<Output = Result<Vec<VmEntity>, DBError>>;

    /// List all VMs under a particular API key, except those that have been deleted
    fn list_by_api_key(
        &self,
        api_key_id: Uuid,
    ) -> impl Future<Output = Result<Vec<VmEntity>, DBError>>;

    /// Count non-deleted VMs that were created from a given commit.
    fn count_by_parent_commit(&self, commit_id: Uuid)
    -> impl Future<Output = Result<i64, DBError>>;

    fn label(
        &self,
        vm_id: &Uuid,
        labels: HashMap<String, String>,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Allocate the next available IPv6 address for a VM in the given account's subnet
    ///
    /// This method calls the database function `next_vm_ip()` which:
    /// 1. Gets the account's /64 subnet from the accounts table
    /// 2. Finds the highest IP currently allocated to VMs in that account
    /// 3. Returns the next sequential IP address
    ///
    /// Example: If account has subnet fd00:fe11:deed:1::/64 and the highest
    /// allocated IP is fd00:fe11:deed:1::5, this returns fd00:fe11:deed:1::6
    fn allocate_vm_ip(&self, account_id: Uuid) -> impl Future<Output = Result<Ipv6Addr, DBError>>;

    fn next_vm_wg_port(&self, chelsea_node_id: Uuid) -> impl Future<Output = Result<u16, DBError>>;

    /// Set (or clear) the node a VM is running on.
    ///
    /// Pass `None` when putting a VM to sleep; pass `Some(node_id)` when waking it.
    fn set_node_id(
        &self,
        vm_id: Uuid,
        node_id: Option<Uuid>,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Update the WireGuard port for a VM.
    ///
    /// Used when waking a VM on a new node where the original port may already be in use.
    fn set_wg_port(&self, vm_id: Uuid, wg_port: u16) -> impl Future<Output = Result<(), DBError>>;
}

#[derive(Debug, Clone)]
pub struct VmEntity {
    pub(crate) vm_id: Uuid,
    /// The commit that this VM was started from, if any.
    pub parent_commit_id: Option<Uuid>,
    /// The VM that this VM's parent commit was created from, if any. Intended to optimize traversing the VM tree.
    pub grandparent_vm_id: Option<Uuid>,
    /// The node this VM is running on. `None` while the VM is sleeping (a `chelsea.sleep_snapshot` row exists).
    pub node_id: Option<Uuid>,
    pub ip: Ipv6Addr,
    pub wg_private_key: String,
    pub wg_public_key: String,
    pub wg_port: u16,
    pub(crate) owner_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub labels: Option<HashMap<String, String>>,
}

impl VmEntity {
    pub fn id(&self) -> Uuid {
        self.vm_id
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }
}

impl From<Row> for VmEntity {
    fn from(row: Row) -> Self {
        let vm_id = row.get("vm_id");
        let wg_private_key = row.get("wg_private_key");
        let wg_public_key = row.get("wg_public_key");
        let ipv6 = match row.get("ip") {
            IpAddr::V6(v6) => v6,
            IpAddr::V4(_) => panic!("VMs should always have a ipv6 address, vm_id = {}", &vm_id),
        };

        let default_label: HashMap<String, String> = HashMap::new();
        let labels = serde_json::from_value(row.get("labels")).unwrap_or(default_label);

        Self {
            vm_id,
            parent_commit_id: row.get("parent_commit_id"),
            grandparent_vm_id: row.get("grandparent_vm_id"),
            node_id: row.get::<_, Option<Uuid>>("node_id"),
            owner_id: row.get("owner_id"),
            wg_public_key,
            wg_private_key,
            // wg_port is stored as INT; postgres doesn't have a u16
            wg_port: row.get::<&str, i32>("wg_port") as u16,
            ip: ipv6,
            labels: Some(labels),
            created_at: row.get("created_at"),
            deleted_at: row.get("deleted_at"),
        }
    }
}

pub struct VMs(DB);

impl DB {
    pub fn vms(&self) -> VMs {
        VMs(self.clone())
    }
}

#[derive(Error, Debug)]
pub enum VMInsertError {
    #[error("db-error: {0:?}")]
    DB(#[from] DBError),
    #[error("not unique node_id/wg_port combination")]
    NotUniqueNodeIdWgPortCombination,
}

impl VMsRepository for VMs {
    async fn insert(
        &self,
        vm_id: Uuid,
        parent_commit_id: Option<Uuid>,
        grandparent_vm_id: Option<Uuid>,
        node_id: Uuid,
        ip: Ipv6Addr,
        wg_private_key: String,
        wg_public_key: String,
        wg_port: u16,
        owner_id: Uuid,
        created_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
        vcpu_count: i32,
        mem_size_mib: i32,
        labels: Option<HashMap<String, String>>,
    ) -> Result<VmEntity, VMInsertError> {
        let rows_res = execute_sql!(
            self.0,
            "INSERT INTO vms (vm_id, parent_commit_id, grandparent_vm_id, node_id, ip, wg_private_key, wg_public_key, wg_port, owner_id, created_at, deleted_at, vcpu_count, mem_size_mib)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
            &[
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::INET,
                Type::TEXT,
                Type::TEXT,
                Type::INT4,
                Type::UUID,
                Type::TIMESTAMPTZ,
                Type::TIMESTAMPTZ,
                Type::INT4,
                Type::INT4
            ],
            &[
                &vm_id,
                &parent_commit_id,
                &grandparent_vm_id,
                &node_id,
                &IpAddr::V6(ip),
                &wg_private_key,
                &wg_public_key,
                &(wg_port as i32),
                &owner_id,
                &created_at,
                &deleted_at,
                &vcpu_count,
                &mem_size_mib,
            ]
        );

        let rows = match rows_res {
            Ok(row) => {
                // Create labels
                for (k, v) in labels.clone().unwrap_or_default() {
                    execute_sql!(
                        self.0,
                        "INSERT INTO labels (vm_id, label_name, label_value)
                         VALUES ($1, $2, $3)",
                        &[Type::UUID, Type::TEXT, Type::TEXT],
                        &[&vm_id, &k, &v]
                    )?;
                }
                row
            }
            Err(err) => match err.as_db_error() {
                Some(db_err)
                    if db_err
                        .constraint()
                        .is_some_and(|constraint| constraint == "wg_port_node_id_pair_unique") =>
                {
                    // This row has a node_id-wg_port that isn't unique in the db. It is
                    // invalid to insert.
                    return Err(VMInsertError::NotUniqueNodeIdWgPortCombination);
                }
                _ => Err(err)?,
            },
        };

        debug_assert!(rows == 1);

        Ok(VmEntity {
            vm_id,
            parent_commit_id,
            grandparent_vm_id,
            node_id: Some(node_id),
            ip,
            owner_id,
            wg_private_key,
            wg_public_key,
            wg_port,
            created_at,
            deleted_at,
            labels,
        })
    }

    async fn list(&self) -> Result<Vec<VmEntity>, DBError> {
        let maybe = query_sql!(
            self.0,
            "SELECT
               vms.*,
               COALESCE(JSON_OBJECT_AGG(labels.label_name, labels.label_value) FILTER (WHERE labels.label_name IS NOT NULL), '{}') as labels
             FROM vms
             LEFT JOIN labels on labels.vm_id = vms.vm_id
             WHERE vms.deleted_at IS NULL
             GROUP BY vms.vm_id"
        )?;
        Ok(maybe.into_iter().map(VmEntity::from).collect())
    }

    async fn get_by_id(&self, vm_id: Uuid) -> Result<Option<VmEntity>, DBError> {
        let maybe = query_one_sql!(
            self.0,
            "SELECT
               vms.*,
               COALESCE(JSON_OBJECT_AGG(labels.label_name, labels.label_value) FILTER (WHERE labels.label_name IS NOT NULL), '{}') as labels
             FROM vms
             LEFT JOIN labels on labels.vm_id = vms.vm_id
             WHERE vms.vm_id = $1 AND vms.deleted_at IS NULL
             GROUP BY vms.vm_id",
            &[Type::UUID],
            &[&vm_id]
        )?;
        Ok(maybe.map(|row| row.try_into().unwrap()))
    }

    async fn list_under_node(&self, node_id: Uuid) -> Result<Vec<VmEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT
               vms.*,
               COALESCE(JSON_OBJECT_AGG(labels.label_name, labels.label_value) FILTER (WHERE labels.label_name IS NOT NULL), '{}') as labels
             FROM vms
             LEFT JOIN labels on labels.vm_id = vms.vm_id
             WHERE vms.node_id = $1 AND vms.deleted_at IS NULL
             GROUP BY vms.vm_id",
            &[Type::UUID],
            &[&node_id]
        )?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn list_grandchild_vms(&self, vm_id: Uuid) -> Result<Vec<VmEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT
               vms.*,
               COALESCE(JSON_OBJECT_AGG(labels.label_name, labels.label_value) FILTER (WHERE labels.label_name IS NOT NULL), '{}') as labels
             FROM vms
             LEFT JOIN labels on labels.vm_id = vms.vm_id
             WHERE vms.grandparent_vm_id = $1 AND vms.deleted_at IS NULL
             GROUP BY vms.vm_id
             ORDER BY created_at DESC",
            &[Type::UUID],
            &[&vm_id]
        )?;
        Ok(rows.into_iter().map(|r| r.try_into().unwrap()).collect())
    }

    async fn mark_deleted(&self, vm_id: &Uuid) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE vms SET deleted_at = Now() WHERE vm_id = $1",
            &[Type::UUID],
            &[vm_id]
        )?;
        debug_assert!(rows == 1);
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    async fn list_by_org_id(&self, org_id: Uuid) -> Result<Vec<VmEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT
               vms.*,
               COALESCE(JSON_OBJECT_AGG(labels.label_name, labels.label_value) FILTER (WHERE labels.label_name IS NOT NULL), '{}') as labels
             FROM vms INNER JOIN api_keys ON api_keys.org_id = $1
             LEFT JOIN labels on labels.vm_id = vms.vm_id
             WHERE vms.owner_id = api_keys.api_key_id AND vms.deleted_at IS NULL
             GROUP BY vms.vm_id",
            &[Type::UUID],
            &[&org_id]
        )?;
        Ok(rows.into_iter().map(|row| row.into()).collect())
    }

    async fn list_by_api_key(&self, api_key_id: Uuid) -> Result<Vec<VmEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT
               vms.*,
               COALESCE(JSON_OBJECT_AGG(labels.label_name, labels.label_value) FILTER (WHERE labels.label_name IS NOT NULL), '{}') as labels
             FROM vms
             LEFT JOIN labels on labels.vm_id = vms.vm_id
             WHERE vms.owner_id = $1 AND vms.deleted_at IS NULL
             GROUP BY vms.vm_id",
            &[Type::UUID],
            &[&api_key_id]
        )?;
        Ok(rows.into_iter().map(|row| row.into()).collect())
    }

    async fn count_by_parent_commit(&self, commit_id: Uuid) -> Result<i64, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT COUNT(*) as count FROM vms WHERE parent_commit_id = $1 AND deleted_at IS NULL",
            &[Type::UUID],
            &[&commit_id]
        )?;

        Ok(row.map(|r| r.get("count")).unwrap_or(0))
    }

    async fn label(&self, vm_id: &Uuid, labels: HashMap<String, String>) -> Result<(), DBError> {
        // Delete all labels for this VM
        execute_sql!(
            self.0,
            "DELETE from labels WHERE vm_id = $1",
            &[Type::UUID],
            &[vm_id]
        )?;

        // Create labels
        for (k, v) in labels {
            execute_sql!(
                self.0,
                "INSERT INTO labels (vm_id, label_name, label_value)
                 VALUES ($1, $2, $3)",
                &[Type::UUID, Type::TEXT, Type::TEXT],
                &[vm_id, &k, &v]
            )?;
        }

        Ok(())
    }

    async fn allocate_vm_ip(&self, account_id: Uuid) -> Result<Ipv6Addr, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT next_vm_ip($1) as ip",
            &[Type::UUID],
            &[&account_id]
        )?
        .expect("next_vm_ip() should always return a row");

        let ip_addr: IpAddr = row.get("ip");

        Ok(match ip_addr {
            IpAddr::V6(v6) => v6,
            IpAddr::V4(_) => panic!("what"),
        })
    }

    async fn next_vm_wg_port(&self, chelsea_node_id: Uuid) -> Result<u16, DBError> {
        let row = query_sql!(
            self.0,
            "SELECT wg_port FROM vms WHERE node_id = $1 AND deleted_at IS NULL ORDER BY wg_port ASC",
            &[Type::UUID],
            &[&chelsea_node_id]
        )?;

        let set: HashSet<u16> = row
            .into_iter()
            .map(|row| row.get::<_, i32>("wg_port").try_into().unwrap())
            .collect();

        const MAX_WG_PORT: u16 = 60_000;

        // Starting port for VM WireGuard interfaces
        // Range: 51830-65535 (reserved for VMs, avoiding 51820-51829 for system use);
        let mut min_wg_port_to_try: u16 = 51_830;

        loop {
            if min_wg_port_to_try > MAX_WG_PORT {
                panic!("No more ports to try.");
            }
            match set.get(&min_wg_port_to_try) {
                Some(_) => {
                    // This port is occupied.
                    min_wg_port_to_try += 1;
                    continue;
                }
                None => {
                    // No VM is using this, free to try to insert to.
                    return Ok(min_wg_port_to_try);
                }
            }
        }

        // for potential_port in 51830..
    }

    async fn set_node_id(&self, vm_id: Uuid, node_id: Option<Uuid>) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE vms SET node_id = $2 WHERE vm_id = $1",
            &[Type::UUID, Type::UUID],
            &[&vm_id, &node_id]
        )?;
        debug_assert!(rows == 1);
        Ok(())
    }

    async fn set_wg_port(&self, vm_id: Uuid, wg_port: u16) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE vms SET wg_port = $2 WHERE vm_id = $1",
            &[Type::UUID, Type::INT4],
            &[&vm_id, &(wg_port as i32)]
        )?;
        debug_assert!(rows == 1);
        Ok(())
    }
}
