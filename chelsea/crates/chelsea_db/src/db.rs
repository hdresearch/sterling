use std::fmt::Display;
use std::path::Path;
use std::sync::Arc;
use std::u32;
use std::{collections::HashSet, net::Ipv4Addr};

use anyhow::Context;
use async_trait::async_trait;
use chelsea_lib::ready_service::VmReadyServiceStore;
use chelsea_lib::ready_service::error::VmReadyServiceStoreError;
use chelsea_lib::vm::VmWireGuardConfig;
use chelsea_lib::vm_manager::types::{VmReservation, VmReservationField};
use chelsea_lib::volume_manager::ceph::{CephVmVolumeManagerStore, CephVmVolumeRecord};
use chelsea_lib::{
    network::VmNetwork,
    network_manager::store::{VmNetworkManagerStore, VmNetworkRecord},
    process_manager::{VmProcessManagerStore, VmProcessRecord},
    store_error::StoreError,
    vm_manager::{VmRecord, store::VmManagerStore},
};
use chrono::{Duration, Utc};
use rusqlite::{Connection, Row, ToSql, params};
use tokio::sync::{Mutex, OnceCell};
use tracing::info;
use uuid::Uuid;
use vers_config::VersConfig;

/// A migration step — either a raw SQL string or a Rust function for
/// migrations that need conditional logic (e.g. column existence checks).
enum Migration {
    Sql(&'static str),
    Fn(fn(&rusqlite::Transaction) -> anyhow::Result<()>),
}

/// Ordered list of migrations. Each entry brings the schema from version N-1
/// to version N. The current schema version is tracked via SQLite's built-in
/// `PRAGMA user_version`.
///
/// To add a new migration:
/// 1. Create a new .sql file in `crates/chelsea_db/migrations/` with a
///    timestamp prefix (e.g. `20260301120000_description.sql`)
/// 2. Append a `Migration::Sql(include_str!(...))` entry to this array
///    (or `Migration::Fn(...)` if you need conditional logic)
const MIGRATIONS: &[Migration] = &[
    Migration::Sql(include_str!("../migrations/20251201000000_base_schema.sql")),
    Migration::Sql(include_str!(
        "../migrations/20251204120000_add_usage_tables.sql"
    )),
    Migration::Sql(include_str!(
        "../migrations/20251212132636_drop_usage_tables.sql"
    )),
    // Migration 4: drop columns that may not exist on nodes that were
    // initialized with the old schema.sql (before the migration system).
    // The old schema.sql didn't include parent_id, size, or current_snap,
    // and migration 1's CREATE TABLE IF NOT EXISTS is a no-op when the
    // tables already exist, so these columns may never have been added.
    Migration::Fn(migration_0004_drop_unused_columns),
    Migration::Sql(include_str!(
        "../migrations/20260227172255_add_node_metadata_table.sql"
    )),
];

/// Check whether a column exists on a table.
fn column_exists(conn: &rusqlite::Transaction, table: &str, column: &str) -> anyhow::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names.iter().any(|n| n == column))
}

/// Drop a column only if it exists. SQLite has no DROP COLUMN IF EXISTS syntax.
fn drop_column_if_exists(
    conn: &rusqlite::Transaction,
    table: &str,
    column: &str,
) -> anyhow::Result<()> {
    if column_exists(conn, table, column)? {
        conn.execute_batch(&format!("ALTER TABLE {} DROP COLUMN {}", table, column))?;
        info!(table, column, "dropped column");
    } else {
        info!(table, column, "column does not exist, skipping drop");
    }
    Ok(())
}

/// Migration 4: drop unused columns (parent_id, size, current_snap).
/// These columns may not exist on nodes initialized with the old schema.sql.
fn migration_0004_drop_unused_columns(tx: &rusqlite::Transaction) -> anyhow::Result<()> {
    drop_column_if_exists(tx, "vm", "parent_id")?;
    drop_column_if_exists(tx, "ceph_vm_volume", "size")?;
    drop_column_if_exists(tx, "ceph_vm_volume", "current_snap")?;
    Ok(())
}

/// Runs all pending migrations on the given connection. Each migration runs in
/// its own transaction and advances `PRAGMA user_version` by one.
fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    let current_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let latest_version = MIGRATIONS.len() as u32;

    if current_version >= latest_version {
        return Ok(());
    }

    info!(
        current_version,
        latest_version, "running chelsea sqlite migrations"
    );

    for (i, migration) in MIGRATIONS.iter().enumerate().skip(current_version as usize) {
        let version = i as u32 + 1;
        let tx = conn.unchecked_transaction()?;
        match migration {
            Migration::Sql(sql) => {
                tx.execute_batch(sql)
                    .with_context(|| format!("failed to run migration {version}"))?;
            }
            Migration::Fn(func) => {
                func(&tx).with_context(|| format!("failed to run migration {version}"))?;
            }
        }
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()
            .with_context(|| format!("failed to commit migration {version}"))?;
        info!(version, "applied migration");
    }

    Ok(())
}

static CHELSEA_DB: OnceCell<Arc<ChelseaDb>> = OnceCell::const_new();

// TEXT is used throughout for UUIDs for human-readability and for scriptability.

/// Recognized values for the `key` column of the Sqlite `node_metadata` table.
pub enum NodeMetadataKey {
    Id,
}

impl Display for NodeMetadataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id => write!(f, "id"),
        }
    }
}

impl ToSql for NodeMetadataKey {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::from(self.to_string()))
    }
}

#[derive(Debug)]
pub struct ChelseaDb {
    connection: Mutex<Connection>,
}

impl ChelseaDb {
    async fn new(db_path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let connection = Connection::open(db_path)?;
        run_migrations(&connection).context("failed to run chelsea sqlite migrations")?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// Create a ChelseaDb at a custom path. For testing only.
    #[doc(hidden)]
    pub async fn new_at_path(db_path: &Path) -> anyhow::Result<Self> {
        Self::new(db_path).await
    }

    /// Returns a reference to a shared ChelseaDb. Will panic on error; a DB init failure is unrecoverable.
    pub async fn instance() -> Arc<ChelseaDb> {
        CHELSEA_DB
            .get_or_init(|| async {
                let config = VersConfig::chelsea();
                let db_path = config.db_path.clone();
                Arc::new(
                    ChelseaDb::new(&db_path)
                        .await
                        .expect("Failed to initialize ChelseaDb"),
                )
            })
            .await
            .clone()
    }

    async fn get_connection(&self) -> tokio::sync::MutexGuard<'_, Connection> {
        self.connection.lock().await
    }

    pub async fn list_all_vm_pids(&self) -> Result<HashSet<u32>, StoreError> {
        let conn = self.get_connection().await;
        let mut stmt = conn
            .prepare("SELECT pid FROM vm_process")
            .map_err(StoreError::from_display)?;
        let rows = stmt
            .query_map(params![], |row| row.get::<_, u32>(0))
            .map_err(StoreError::from_display)?;

        rows.collect::<Result<HashSet<_>, _>>()
            .map_err(StoreError::from_display)
    }

    /// Fetches the value for a given key from the node_metadata table.
    pub async fn fetch_node_metadata_value(
        &self,
        key: &NodeMetadataKey,
    ) -> Result<Option<String>, StoreError> {
        let conn = self.get_connection().await;
        let mut stmt = conn
            .prepare("SELECT value FROM node_metadata WHERE key = ?1")
            .map_err(StoreError::from_display)?;

        let mut rows = stmt
            .query_map(params![key], |row| row.get::<_, String>(0))
            .map_err(StoreError::from_display)?;

        if let Some(result) = rows.next() {
            match result {
                Ok(val) => Ok(Some(val)),
                Err(e) => Err(StoreError::from_display(e)),
            }
        } else {
            Ok(None)
        }
    }

    /// Inserts or updates a key-value pair in the node_metadata table.
    pub async fn set_node_metadata_value(
        &self,
        key: &NodeMetadataKey,
        value: &str,
    ) -> Result<(), StoreError> {
        let conn = self.get_connection().await;
        conn.execute(
            "INSERT INTO node_metadata (key, value) VALUES (?1, ?2)
                ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )
        .map_err(StoreError::from_display)?;
        Ok(())
    }

    /// Fetches the node ID from the node_metadata table, creating it if it doesn't exist.
    pub async fn get_or_create_node_id(&self) -> Result<Uuid, StoreError> {
        match self.fetch_node_metadata_value(&NodeMetadataKey::Id).await? {
            Some(id) => id.parse().map_err(StoreError::from_display),
            None => {
                let id = Uuid::new_v4();
                self.set_node_metadata_value(&NodeMetadataKey::Id, id.to_string().as_str())
                    .await?;
                Ok(id)
            }
        }
    }
}

#[async_trait]
impl VmNetworkManagerStore for ChelseaDb {
    async fn insert_vm_network(&self, vm_network: VmNetworkRecord) -> Result<(), StoreError> {
        self.get_connection()
            .await
            .execute(
                "INSERT INTO vm_network (
                host_addr, vm_addr, netns_name, ssh_port,
                wg_interface_name, wg_private_key, wg_private_ip,
                wg_peer_pub_key, wg_peer_pub_ip, wg_peer_prv_ip, wg_port,
                reserved_until
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    vm_network.host_addr,
                    vm_network.vm_addr,
                    vm_network.netns_name,
                    vm_network.ssh_port,
                    vm_network.wg.clone().map(|wg| wg.interface_name),
                    vm_network.wg.clone().map(|wg| wg.private_key),
                    vm_network.wg.clone().map(|wg| wg.private_ip.to_string()),
                    vm_network.wg.clone().map(|wg| wg.peer_pub_key),
                    vm_network.wg.clone().map(|wg| wg.peer_pub_ip.to_string()),
                    vm_network.wg.clone().map(|wg| wg.peer_ipv6.to_string()),
                    vm_network.wg.clone().map(|wg| wg.wg_port),
                    vm_network.reserved_until,
                ],
            )
            .map_err(StoreError::from_display)?;

        Ok(())
    }

    async fn set_wg_on_vm_network(
        &self,
        host_addr: &Ipv4Addr,
        wg: Option<VmWireGuardConfig>,
    ) -> Result<Option<()>, StoreError> {
        let conn = self.get_connection().await;
        match wg {
            Some(config) => {
                let mut stmt = conn
                    .prepare(
                        "UPDATE
                       vm_network
                     SET
                       wg_interface_name = ?1, wg_private_key = ?2, wg_private_ip = ?3,
                       wg_peer_pub_key = ?4, wg_peer_pub_ip = ?5, wg_peer_prv_ip = ?6,
                       wg_port = ?7
                     WHERE host_addr = ?8",
                    )
                    .map_err(StoreError::from_display)?;

                let rows = stmt
                    .execute(params![
                        &config.interface_name,
                        &config.private_key,
                        &config.private_ip.to_string(),
                        &config.peer_pub_key,
                        config.peer_pub_ip.to_string(),
                        config.peer_ipv6.to_string(),
                        config.wg_port,
                        host_addr.to_bits()
                    ])
                    .map_err(StoreError::from_display)?;

                Ok(if rows == 0 { None } else { Some(()) })
            }
            None => {
                let mut stmt = conn
                    .prepare(
                        "UPDATE
                       vm_network
                     SET
                       wg_interface_name = NULL, wg_private_key = NULL, wg_private_ip = NULL,
                       wg_peer_pub_key = NULL, wg_peer_pub_ip = NULL, wg_peer_prv_ip = NULL,
                       wg_port = NULL
                     WHERE host_addr = ?1",
                    )
                    .map_err(StoreError::from_display)?;

                let rows = stmt
                    .execute(params![host_addr.to_bits()])
                    .map_err(StoreError::from_display)?;
                Ok(if rows == 0 { None } else { Some(()) })
            }
        }
    }

    async fn fetch_vm_network(
        &self,
        host_addr: &Ipv4Addr,
    ) -> Result<Option<VmNetwork>, StoreError> {
        let host_addr = host_addr.to_bits();
        let conn = self.get_connection().await;

        let mut stmt = conn
            .prepare(
                "SELECT vm_addr, netns_name, ssh_port,
                wg_interface_name, wg_private_key, wg_private_ip,
                wg_peer_pub_key, wg_peer_pub_ip, wg_peer_prv_ip, wg_port,
                reserved_until
             FROM vm_network WHERE host_addr = ?1",
            )
            .map_err(StoreError::from_display)?;

        let mut rows = stmt
            .query(params![host_addr])
            .map_err(StoreError::from_display)?;

        if let Some(row) = rows.next().map_err(StoreError::from_display)? {
            fn into_wg(row: &Row<'_>) -> Option<VmWireGuardConfig> {
                // If any of these fields don't exist, it's considered that the wg config doesn't
                // exist at all.
                let private_ip_str = row.get::<_, String>(5).ok()?;
                let peer_pub_ip_str = row.get::<_, String>(7).ok()?;
                let peer_ipv6_str = row.get::<_, String>(8).ok()?;

                Some(VmWireGuardConfig {
                    interface_name: row.get(3).ok()?,
                    private_key: row.get(4).ok()?,
                    private_ip: private_ip_str.parse().expect("this cannot fail if we maintain to only insert valid ipv6 addresses in this column"),
                    peer_pub_key: row.get(6).ok()?,
                    peer_pub_ip: peer_pub_ip_str.parse().expect("this cannot fail if we maintain to only insert valid ipv4 addresses in this column"),
                    peer_ipv6: peer_ipv6_str.parse().expect("this cannot fail if we maintain to only insert valid ipv6 addresses in this column"),
                    wg_port: row.get(9).ok()?
                })
            }
            Ok(Some(VmNetwork::from(VmNetworkRecord {
                host_addr,
                vm_addr: row.get(0).map_err(StoreError::from_display)?,
                netns_name: row.get(1).map_err(StoreError::from_display)?,
                ssh_port: row.get(2).map_err(StoreError::from_display)?,
                wg: into_wg(&row),
                reserved_until: row.get(10).map_err(StoreError::from_display)?,
            })))
        } else {
            Ok(None)
        }
    }

    async fn check_vm_network_exists(
        &self,
        host_addr: &std::net::Ipv4Addr,
    ) -> Result<bool, StoreError> {
        let host_addr = host_addr.to_bits();
        let conn = self.get_connection().await;

        let mut stmt = conn
            .prepare("SELECT EXISTS(SELECT 1 FROM vm_network WHERE host_addr = ?1)")
            .map_err(StoreError::from_display)?;

        let mut rows = stmt
            .query(params![host_addr])
            .map_err(StoreError::from_display)?;

        if let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let exists: i32 = row.get(0).map_err(StoreError::from_display)?;
            Ok(exists == 1)
        } else {
            Ok(false)
        }
    }

    async fn delete_vm_network(&self, host_addr: &Ipv4Addr) -> Result<(), StoreError> {
        let host_addr = host_addr.to_bits();
        self.get_connection()
            .await
            .execute(
                "DELETE FROM vm_network WHERE host_addr = ?1",
                params![host_addr],
            )
            .map_err(StoreError::from_display)?;

        Ok(())
    }

    /// Finds an available network such that:
    /// 1) The current time is not before reserved_until,
    /// 2) There is no VM whose network_host_addr is equal to this network's host_addr
    /// And updates its reserved_until time to be {VersConfig::network_reserve_timeout_secs} from now
    async fn reserve_network(&self) -> Result<Option<VmNetwork>, StoreError> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let conn = self.get_connection().await;

        let reserve_for = {
            let config = VersConfig::chelsea();
            let timeout = config.network_reserve_timeout_secs as i64;
            timeout
        };
        let new_reserved_until = (now + Duration::seconds(reserve_for.into())).to_rfc3339();

        let mut stmt = conn.prepare(
            "SELECT host_addr, vm_addr, netns_name, ssh_port, wg_interface_name, wg_private_key, wg_private_ip,
            wg_peer_pub_key, wg_peer_pub_ip, wg_peer_prv_ip, wg_port, reserved_until
             FROM vm_network
             WHERE reserved_until <= ?1
             AND host_addr NOT IN (
                 SELECT vm_network_host_addr FROM vm
             )
             LIMIT 1",
        ).map_err(StoreError::from_display)?;

        let mut rows = stmt
            .query(params![now_str])
            .map_err(StoreError::from_display)?;

        if let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let host_addr: u32 = row.get(0).map_err(StoreError::from_display)?;
            // Update reserved_until for this record
            conn.execute(
                "UPDATE vm_network SET reserved_until = ?1 WHERE host_addr = ?2",
                params![new_reserved_until, host_addr],
            )
            .map_err(StoreError::from_display)?;

            fn into_wg(row: &Row<'_>) -> Option<VmWireGuardConfig> {
                // If any of these fields don't exist, it's considered that the wg config doesn't
                // exist at all.
                Some(VmWireGuardConfig {
                    interface_name: row.get(4).ok()?,
                    private_key: row.get(5).ok()?,
                    private_ip: row.get::<_, String>(6).ok()?.parse().expect("this cannot fail if we maintain to only insert valid ipv6 addresses in this column"),
                    peer_pub_key: row.get(7).ok()?,
                    peer_pub_ip: row.get::<_, String>(8).ok()?.parse().expect("this cannot fail if we maintain to only insert valid ipv4 addresses in this column"),
                    peer_ipv6: row.get::<_, String>(9).ok()?.parse().expect("this cannot fail if we maintain to only insert valid ipv6 addresses in this column"),
                    wg_port: row.get(10).ok()?
                })
            }

            Ok(Some(VmNetwork::from(VmNetworkRecord {
                host_addr,
                vm_addr: row.get(1).map_err(StoreError::from_display)?,
                netns_name: row.get(2).map_err(StoreError::from_display)?,
                ssh_port: row.get(3).map_err(StoreError::from_display)?,
                wg: into_wg(&row),
                reserved_until: new_reserved_until,
            })))
        } else {
            Ok(None)
        }
    }

    async fn unreserve_network(&self, host_addr: &Ipv4Addr) -> Result<(), StoreError> {
        let host_addr = host_addr.to_bits();
        let now = (Utc::now() - Duration::seconds(1)).to_rfc3339();
        self.get_connection()
            .await
            .execute(
                "UPDATE vm_network SET reserved_until = ?1 WHERE host_addr = ?2",
                params![now, host_addr],
            )
            .map_err(StoreError::from_display)?;
        Ok(())
    }
}

#[async_trait]
impl VmManagerStore for ChelseaDb {
    async fn get_vm_vcpu_and_ram_usage(&self) -> Result<(u32, u32), StoreError> {
        let result = self
            .get_connection()
            .await
            .query_row(
                "SELECT vcpu_count_sum, mem_size_mib_sum FROM vm_sum",
                params![],
                |row| {
                    Ok((
                        row.get::<_, Option<u32>>(0)?.unwrap_or(0),
                        row.get::<_, Option<u32>>(1)?.unwrap_or(0),
                    ))
                },
            )
            .map_err(StoreError::from_display)?;
        Ok(result)
    }

    async fn insert_vm_record(&self, vm: VmRecord) -> Result<(), StoreError> {
        let vm_network_host_addr = vm.vm_network_host_addr.to_bits();
        let connection = self.get_connection().await;
        connection.execute(
            "INSERT INTO vm (id, ssh_public_key, ssh_private_key, kernel_name, image_name, vcpu_count, mem_size_mib, fs_size_mib, vm_network_host_addr, vm_process_pid, vm_volume_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                vm.id.to_string(),
                vm.ssh_public_key,
                vm.ssh_private_key,
                vm.kernel_name,
                vm.image_name,
                vm.vcpu_count,
                vm.mem_size_mib,
                vm.fs_size_mib,
                vm_network_host_addr,
                vm.vm_process_pid,
                vm.vm_volume_id.to_string()
            ],
        ).map_err(StoreError::from_display)?;

        Ok(())
    }

    async fn fetch_vm_record(&self, id: &Uuid) -> Result<Option<VmRecord>, StoreError> {
        let connection = self.get_connection().await;
        let mut stmt = connection.prepare(
        "SELECT id, ssh_public_key, ssh_private_key, kernel_name, image_name, vcpu_count, mem_size_mib, fs_size_mib, vm_network_host_addr, vm_process_pid, vm_volume_id FROM vm WHERE id = ?1"
    ).map_err(StoreError::from_display)?;

        let mut rows = stmt
            .query(params![id.to_string()])
            .map_err(StoreError::from_display)?;

        if let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let id_str: String = row.get(0).map_err(StoreError::from_display)?;
            let vm_volume_id_str: String = row.get(10).map_err(StoreError::from_display)?;

            Ok(Some(VmRecord {
                id: Uuid::parse_str(&id_str).map_err(StoreError::from_display)?,
                ssh_public_key: row.get(1).map_err(StoreError::from_display)?,
                ssh_private_key: row.get(2).map_err(StoreError::from_display)?,
                kernel_name: row.get(3).map_err(StoreError::from_display)?,
                image_name: row.get(4).map_err(StoreError::from_display)?,
                vcpu_count: row.get(5).map_err(StoreError::from_display)?,
                mem_size_mib: row.get(6).map_err(StoreError::from_display)?,
                fs_size_mib: row.get(7).map_err(StoreError::from_display)?,
                vm_network_host_addr: Ipv4Addr::from_bits(
                    row.get(8).map_err(StoreError::from_display)?,
                ),
                vm_process_pid: row.get(9).map_err(StoreError::from_display)?,
                vm_volume_id: Uuid::parse_str(&vm_volume_id_str)
                    .map_err(StoreError::from_display)?,
            }))
        } else {
            Ok(None)
        }
    }

    async fn delete_vm_record(&self, id: &Uuid) -> Result<(), StoreError> {
        let connection = self.get_connection().await;
        let _deleted_rows = connection
            .execute("DELETE FROM vm WHERE id = ?1", params![id.to_string()])
            .map_err(StoreError::from_display)?;
        Ok(())
    }

    async fn list_all_vm_ids(&self) -> Result<Vec<Uuid>, StoreError> {
        let connection = self.get_connection().await;
        let mut stmt = connection
            .prepare("SELECT id FROM vm")
            .map_err(StoreError::from_display)?;
        let mut rows = stmt.query([]).map_err(StoreError::from_display)?;
        let mut vm_ids = Vec::new();
        while let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let id_str: String = row.get(0).map_err(StoreError::from_display)?;
            let id = Uuid::parse_str(&id_str).map_err(StoreError::from_display)?;
            vm_ids.push(id);
        }
        Ok(vm_ids)
    }

    async fn list_all_vms_with_pids(&self) -> Result<Vec<(Uuid, u32)>, StoreError> {
        let connection = self.get_connection().await;
        let mut stmt = connection
            .prepare("SELECT id, vm_process_pid FROM vm")
            .map_err(StoreError::from_display)?;
        let mut rows = stmt.query([]).map_err(StoreError::from_display)?;

        let mut vms_with_pids = Vec::new();
        while let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let vm_id = row
                .get::<_, String>(0)
                .map_err(StoreError::from_display)?
                .parse()
                .map_err(StoreError::from_display)?;
            let pid = row.get(1).map_err(StoreError::from_display)?;
            vms_with_pids.push((vm_id, pid));
        }

        Ok(vms_with_pids)
    }

    async fn count_vms(&self) -> Result<u64, StoreError> {
        self.get_connection()
            .await
            .prepare("SELECT COUNT(*) FROM vm")
            .map_err(StoreError::from_display)?
            .query_row([], |row| row.get(0))
            .map_err(StoreError::from_display)
    }

    async fn fetch_vm_with_network(
        &self,
        id: &Uuid,
    ) -> Result<Option<(VmRecord, Option<VmNetwork>)>, StoreError> {
        let conn = self.get_connection().await;
        let mut stmt = conn
            .prepare(
                "SELECT
                v.id, v.ssh_public_key, v.ssh_private_key,
                v.kernel_name, v.image_name, v.vcpu_count, v.mem_size_mib,
                v.fs_size_mib, v.vm_network_host_addr, v.vm_process_pid, v.vm_volume_id,
                n.host_addr, n.vm_addr, n.netns_name, n.ssh_port,
                n.wg_interface_name, n.wg_private_key, n.wg_private_ip,
                n.wg_peer_pub_key, n.wg_peer_pub_ip, n.wg_peer_prv_ip, n.wg_port, n.reserved_until
            FROM vm v
            LEFT JOIN vm_network n ON v.vm_network_host_addr = n.host_addr
            WHERE v.id = ?1",
            )
            .map_err(StoreError::from_display)?;

        let mut rows = stmt
            .query(params![id.to_string()])
            .map_err(StoreError::from_display)?;

        if let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let id_str: String = row.get(0).map_err(StoreError::from_display)?;
            let vm_volume_id_str: String = row.get(10).map_err(StoreError::from_display)?;

            let vm_record = VmRecord {
                id: Uuid::parse_str(&id_str).map_err(StoreError::from_display)?,
                ssh_public_key: row.get(1).map_err(StoreError::from_display)?,
                ssh_private_key: row.get(2).map_err(StoreError::from_display)?,
                kernel_name: row.get(3).map_err(StoreError::from_display)?,
                image_name: row.get(4).map_err(StoreError::from_display)?,
                vcpu_count: row.get(5).map_err(StoreError::from_display)?,
                mem_size_mib: row.get(6).map_err(StoreError::from_display)?,
                fs_size_mib: row.get(7).map_err(StoreError::from_display)?,
                vm_network_host_addr: Ipv4Addr::from_bits(
                    row.get(8).map_err(StoreError::from_display)?,
                ),
                vm_process_pid: row.get(9).map_err(StoreError::from_display)?,
                vm_volume_id: Uuid::parse_str(&vm_volume_id_str)
                    .map_err(StoreError::from_display)?,
            };

            let host_addr_bits: Option<u32> = row.get(11).map_err(StoreError::from_display)?;
            let vm_addr_bits: Option<u32> = row.get(12).map_err(StoreError::from_display)?;
            let netns_name: Option<String> = row.get(13).map_err(StoreError::from_display)?;
            let ssh_port: Option<u16> = row.get(14).map_err(StoreError::from_display)?;

            let network = if let (
                Some(host_addr_bits),
                Some(vm_addr_bits),
                Some(netns_name),
                Some(ssh_port),
            ) = (host_addr_bits, vm_addr_bits, netns_name, ssh_port)
            {
                let wg = match (
                    row.get::<_, Option<String>>(15)
                        .map_err(StoreError::from_display)?,
                    row.get::<_, Option<String>>(16)
                        .map_err(StoreError::from_display)?,
                    row.get::<_, Option<String>>(17)
                        .map_err(StoreError::from_display)?,
                    row.get::<_, Option<String>>(18)
                        .map_err(StoreError::from_display)?,
                    row.get::<_, Option<String>>(19)
                        .map_err(StoreError::from_display)?,
                    row.get::<_, Option<String>>(20)
                        .map_err(StoreError::from_display)?,
                ) {
                    (
                        Some(interface_name),
                        Some(private_key),
                        Some(private_ip),
                        Some(peer_pub_key),
                        Some(peer_pub_ip),
                        Some(peer_prv_ip),
                    ) => Some(VmWireGuardConfig {
                        interface_name,
                        private_key,
                        private_ip: private_ip
                            .parse()
                            .context("invalid WireGuard private IPv6 address")
                            .map_err(StoreError::from_display)?,
                        peer_pub_key,
                        peer_pub_ip: peer_pub_ip
                            .parse()
                            .context("invalid WireGuard peer IPv4 address")
                            .map_err(StoreError::from_display)?,
                        peer_ipv6: peer_prv_ip
                            .parse()
                            .context("invalid WireGuard peer IPv6 address")
                            .map_err(StoreError::from_display)?,
                        wg_port: row.get(21).map_err(StoreError::from_display)?,
                    }),
                    _ => None,
                };

                let reserved_until = row
                    .get::<_, Option<String>>(22)
                    .map_err(StoreError::from_display)?
                    .unwrap_or_else(|| Utc::now().to_rfc3339());

                let record = VmNetworkRecord {
                    host_addr: host_addr_bits,
                    vm_addr: vm_addr_bits,
                    netns_name,
                    ssh_port,
                    wg,
                    reserved_until,
                };

                Some(VmNetwork::from(record))
            } else {
                None
            };

            Ok(Some((vm_record, network)))
        } else {
            Ok(None)
        }
    }

    async fn update_vm_process_pid(&self, id: &Uuid, pid: u32) -> Result<(), StoreError> {
        self.get_connection()
            .await
            .execute(
                "UPDATE vm SET vm_process_pid = ?1 WHERE id = ?2",
                params![pid, id.to_string()],
            )
            .map_err(StoreError::from_display)?;
        Ok(())
    }

    async fn update_vm_fs_size_mib(&self, id: &Uuid, fs_size_mib: u32) -> Result<(), StoreError> {
        self.get_connection()
            .await
            .execute(
                "UPDATE vm SET fs_size_mib = ?1 WHERE id = ?2",
                params![fs_size_mib, id.to_string()],
            )
            .map_err(StoreError::from_display)?;
        Ok(())
    }

    async fn get_vm_resource_reservation(&self) -> Result<VmReservation, StoreError> {
        // Query the vm_sum view to get the sum of vcpu_count and mem_size_mib currently allocated to running VMs.
        let (vcpu_count_used, memory_mib_used) = self.get_vm_vcpu_and_ram_usage().await?;

        let config = VersConfig::chelsea();

        // Special handling for the value 0 when overprovisioning is enabled
        let vcpu_count_total = match config.allow_vcpu_overprovisioning {
            true => match config.vm_total_vcpu_count {
                0 => u32::MAX,
                other => other,
            },
            false => config.vm_total_vcpu_count,
        };
        let memory_mib_total = match config.allow_memory_overprovisioning {
            true => match config.vm_total_memory_mib {
                0 => u32::MAX,
                other => other,
            },
            false => config.vm_total_memory_mib,
        };

        Ok(VmReservation {
            vcpu_count: VmReservationField {
                max: config.vm_max_vcpu_count,
                total: vcpu_count_total,
                used: vcpu_count_used,
            },
            memory_mib: VmReservationField {
                max: config.vm_max_memory_mib,
                total: memory_mib_total,
                used: memory_mib_used,
            },
            volume_mib: VmReservationField {
                max: config.vm_max_volume_mib,
                // We currently regard our storage backend as functionally infinite
                total: u32::MAX,
                used: 0,
            },
        })
    }
}

#[async_trait]
impl VmProcessManagerStore for ChelseaDb {
    async fn insert_vm_process_record(
        &self,
        vm_process: &VmProcessRecord,
    ) -> Result<(), StoreError> {
        self.get_connection()
            .await
            .execute(
                "INSERT INTO vm_process (pid, process_type, vm_id) VALUES (?1, ?2, ?3)",
                params![
                    vm_process.pid,
                    vm_process.process_type.to_string(),
                    vm_process.vm_id.to_string()
                ],
            )
            .map_err(StoreError::from_display)?;

        Ok(())
    }

    async fn fetch_vm_process_record(
        &self,
        id: u32,
    ) -> Result<Option<VmProcessRecord>, StoreError> {
        let connection = self.get_connection().await;
        let mut stmt = connection
            .prepare("SELECT pid, process_type, vm_id FROM vm_process WHERE pid = ?1")
            .map_err(StoreError::from_display)?;

        let mut rows = stmt.query(params![id]).map_err(StoreError::from_display)?;

        if let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let pid: u32 = row.get(0).map_err(StoreError::from_display)?;
            let process_type_str: String = row.get(1).map_err(StoreError::from_display)?;
            let process_type = process_type_str.parse().map_err(StoreError::from_display)?;
            let vm_id_str: String = row.get(2).map_err(StoreError::from_display)?;
            let vm_id = Uuid::parse_str(&vm_id_str).map_err(StoreError::from_display)?;

            Ok(Some(VmProcessRecord {
                pid,
                process_type,
                vm_id,
            }))
        } else {
            Ok(None)
        }
    }

    async fn delete_vm_process_record(&self, pid: u32) -> Result<(), StoreError> {
        self.get_connection()
            .await
            .execute("DELETE FROM vm_process WHERE pid = ?1", params![pid])
            .map_err(StoreError::from_display)?;
        Ok(())
    }
}

#[async_trait]
impl CephVmVolumeManagerStore for ChelseaDb {
    async fn insert_ceph_vm_volume_record(
        &self,
        record: CephVmVolumeRecord,
    ) -> Result<(), StoreError> {
        self.get_connection()
            .await
            .execute(
                "INSERT INTO ceph_vm_volume (id, image_name, device_path) VALUES (?1, ?2, ?3)",
                params![record.id.to_string(), record.image_name, record.device_path,],
            )
            .map_err(StoreError::from_display)?;

        Ok(())
    }

    async fn fetch_ceph_vm_volume_record(
        &self,
        vm_volume_id: &Uuid,
    ) -> Result<Option<CephVmVolumeRecord>, StoreError> {
        let connection = self.get_connection().await;
        let mut stmt = connection
            .prepare("SELECT id, image_name, device_path FROM ceph_vm_volume WHERE id = ?1")
            .map_err(StoreError::from_display)?;

        let mut rows = stmt
            .query(params![vm_volume_id.to_string()])
            .map_err(StoreError::from_display)?;

        if let Some(row) = rows.next().map_err(StoreError::from_display)? {
            let id_str: String = row.get(0).map_err(StoreError::from_display)?;
            let id = Uuid::parse_str(&id_str).map_err(StoreError::from_display)?;
            let image_name: String = row.get(1).map_err(StoreError::from_display)?;
            let device_path: String = row.get(2).map_err(StoreError::from_display)?;
            Ok(Some(CephVmVolumeRecord {
                id,
                image_name,
                device_path,
            }))
        } else {
            Ok(None)
        }
    }

    async fn delete_ceph_vm_volume_record(&self, vm_volume_id: &Uuid) -> Result<(), StoreError> {
        self.get_connection()
            .await
            .execute(
                "DELETE FROM ceph_vm_volume WHERE id = ?1",
                params![vm_volume_id.to_string()],
            )
            .map_err(StoreError::from_display)?;
        Ok(())
    }
}

#[async_trait]
impl VmReadyServiceStore for ChelseaDb {
    async fn vm_exists(&self, vm_id: &Uuid) -> Result<bool, VmReadyServiceStoreError> {
        let conn = self.get_connection().await;
        let mut stmt = conn
            .prepare("SELECT EXISTS(SELECT 1 FROM vm WHERE id = ?1)")
            .map_err(|e| VmReadyServiceStoreError::Db(e.to_string()))?;

        let exists: i32 = stmt
            .query_row(params![vm_id.to_string()], |row| row.get(0))
            .map_err(|e| VmReadyServiceStoreError::Db(e.to_string()))?;

        Ok(exists == 1)
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use chrono::{Duration, Utc};
    use tempfile::NamedTempFile;
    use tokio::sync::OnceCell;
    use util_test::env::*;
    use vers_config::HypervisorType;

    use super::*;

    async fn init_test_env() {
        static INIT: OnceCell<()> = OnceCell::const_new();
        INIT.get_or_init(|| async {
            // Set default environment variables
            let _g = env_lock().await;
            let vars = default_env_vars();
            set_env(&vars);
        })
        .await;
    }

    /// Helper: get the user_version pragma from a connection.
    fn get_user_version(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    /// Helper: get column names for a table.
    fn get_column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// Helper: check if a table exists.
    fn table_exists(conn: &Connection, table: &str) -> bool {
        conn.prepare(&format!(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='{}'",
            table
        ))
        .unwrap()
        .exists([])
        .unwrap()
    }

    #[test]
    fn test_migrations_on_fresh_db() {
        let db_file = NamedTempFile::new().unwrap();
        let conn = Connection::open(db_file.path()).unwrap();

        run_migrations(&conn).unwrap();

        // Should be at latest version
        assert_eq!(get_user_version(&conn), MIGRATIONS.len() as i64);

        // Core tables exist
        assert!(table_exists(&conn, "vm"));
        assert!(table_exists(&conn, "vm_network"));
        assert!(table_exists(&conn, "vm_process"));
        assert!(table_exists(&conn, "ceph_vm_volume"));

        // Usage tables should NOT exist (added then dropped)
        assert!(!table_exists(&conn, "vm_start"));
        assert!(!table_exists(&conn, "vm_stop"));
        assert!(!table_exists(&conn, "usage_reporting_state"));

        // Dropped columns should not be present
        let vm_cols = get_column_names(&conn, "vm");
        assert!(!vm_cols.contains(&"parent_id".to_string()));

        let vol_cols = get_column_names(&conn, "ceph_vm_volume");
        assert!(!vol_cols.contains(&"size".to_string()));
        assert!(!vol_cols.contains(&"current_snap".to_string()));
    }

    #[test]
    fn test_migrations_reopen_is_noop() {
        let db_file = NamedTempFile::new().unwrap();

        // First open: run all migrations
        {
            let conn = Connection::open(db_file.path()).unwrap();
            run_migrations(&conn).unwrap();
            assert_eq!(get_user_version(&conn), MIGRATIONS.len() as i64);
        }

        // Second open: run_migrations should be a no-op
        {
            let conn = Connection::open(db_file.path()).unwrap();
            run_migrations(&conn).unwrap();
            assert_eq!(get_user_version(&conn), MIGRATIONS.len() as i64);

            // Schema still correct
            assert!(table_exists(&conn, "vm"));
            assert!(!table_exists(&conn, "vm_start"));
        }
    }

    #[tokio::test]
    async fn test_list_all_vm_pids_returns_inserted_pids() {
        init_test_env().await;
        let db_file = NamedTempFile::new().unwrap();
        let db = ChelseaDb::new(db_file.path()).await.unwrap();

        // Insert a few vm_process rows directly
        db.insert_vm_process_record(&VmProcessRecord {
            pid: 1001,
            process_type: HypervisorType::Firecracker,
            vm_id: Uuid::new_v4(),
        })
        .await
        .unwrap();
        db.insert_vm_process_record(&VmProcessRecord {
            pid: 1002,
            process_type: HypervisorType::Firecracker,
            vm_id: Uuid::new_v4(),
        })
        .await
        .unwrap();
        db.insert_vm_process_record(&VmProcessRecord {
            pid: u32::MAX,
            process_type: HypervisorType::Firecracker,
            vm_id: Uuid::new_v4(),
        })
        .await
        .unwrap();

        let pids = db.list_all_vm_pids().await.unwrap();
        println!("{pids:?}");
        assert!(pids.contains(&1001));
        assert!(pids.contains(&1002));
        assert!(pids.contains(&u32::MAX));
        assert_eq!(pids.len(), 3);
    }

    #[tokio::test]
    #[ignore = "requires VersConfig (/etc/vers) — run on SNE nodes only"]
    async fn test_reserve_network_updates_and_skips_second_attempt() {
        init_test_env().await;
        let db_file = NamedTempFile::new().unwrap();
        let db = ChelseaDb::new(db_file.path()).await.unwrap();

        let host_addr = Ipv4Addr::new(10, 0, 0, 2);
        db.insert_vm_network(VmNetworkRecord {
            host_addr: host_addr.to_bits(),
            vm_addr: Ipv4Addr::new(10, 0, 0, 3).to_bits(),
            netns_name: "ns-test".to_string(),
            ssh_port: 28001,
            wg: None,
            reserved_until: (Utc::now() - Duration::seconds(60)).to_rfc3339(),
        })
        .await
        .unwrap();

        let reserved = db.reserve_network().await.unwrap();
        assert!(reserved.is_some());
        let reserved_network = reserved.unwrap();
        assert_eq!(reserved_network.host_addr, host_addr);
        assert_eq!(reserved_network.netns_name, "ns-test");
        assert_eq!(reserved_network.ssh_port, 28001);

        // After reserving once, the entry should no longer qualify until the timeout elapses.
        let second_attempt = db.reserve_network().await.unwrap();
        assert!(second_attempt.is_none());
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn test_vm_network_record(host: Ipv4Addr, vm: Ipv4Addr) -> VmNetworkRecord {
        VmNetworkRecord {
            host_addr: host.to_bits(),
            vm_addr: vm.to_bits(),
            netns_name: format!("ns-{}", host),
            ssh_port: 28001,
            wg: None,
            reserved_until: (Utc::now() - Duration::seconds(60)).to_rfc3339(),
        }
    }

    fn test_vm_record(network_host: Ipv4Addr) -> VmRecord {
        VmRecord {
            id: Uuid::new_v4(),
            ssh_public_key: "ssh-rsa AAAA...".to_string(),
            ssh_private_key: "-----BEGIN RSA PRIVATE KEY-----\n...".to_string(),
            kernel_name: "default.bin".to_string(),
            image_name: "default".to_string(),
            vcpu_count: 1,
            mem_size_mib: 512,
            fs_size_mib: 1024,
            vm_network_host_addr: network_host,
            vm_process_pid: 9999,
            vm_volume_id: Uuid::new_v4(),
        }
    }

    async fn make_db() -> (ChelseaDb, NamedTempFile) {
        init_test_env().await;
        let f = NamedTempFile::new().unwrap();
        let db = ChelseaDb::new(f.path()).await.unwrap();
        (db, f)
    }

    // ── VmNetwork CRUD ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_insert_and_fetch_vm_network() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 0, 10);
        let rec = test_vm_network_record(host, Ipv4Addr::new(10, 0, 0, 11));
        db.insert_vm_network(rec).await.unwrap();

        let fetched = db.fetch_vm_network(&host).await.unwrap();
        assert!(fetched.is_some());
        let net = fetched.unwrap();
        assert_eq!(net.host_addr, host);
        assert_eq!(net.netns_name, format!("ns-{host}"));
        assert!(net.wg.is_none());
    }

    #[tokio::test]
    async fn test_fetch_vm_network_nonexistent_returns_none() {
        let (db, _f) = make_db().await;
        let result = db
            .fetch_vm_network(&Ipv4Addr::new(1, 2, 3, 4))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_check_vm_network_exists() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 1, 1);
        assert!(!db.check_vm_network_exists(&host).await.unwrap());

        db.insert_vm_network(test_vm_network_record(host, Ipv4Addr::new(10, 0, 1, 2)))
            .await
            .unwrap();
        assert!(db.check_vm_network_exists(&host).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_vm_network() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 2, 1);
        db.insert_vm_network(test_vm_network_record(host, Ipv4Addr::new(10, 0, 2, 2)))
            .await
            .unwrap();

        db.delete_vm_network(&host).await.unwrap();
        assert!(!db.check_vm_network_exists(&host).await.unwrap());
    }

    #[tokio::test]
    #[ignore = "requires VersConfig (/etc/vers) — run on SNE nodes only"]
    async fn test_unreserve_network_makes_it_available_again() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 3, 1);
        db.insert_vm_network(test_vm_network_record(host, Ipv4Addr::new(10, 0, 3, 2)))
            .await
            .unwrap();

        // Reserve it
        let first = db.reserve_network().await.unwrap();
        assert!(first.is_some());
        // Should be unavailable now
        assert!(db.reserve_network().await.unwrap().is_none());

        // Unreserve
        db.unreserve_network(&host).await.unwrap();
        // Should be available again
        let again = db.reserve_network().await.unwrap();
        assert!(again.is_some());
    }

    #[tokio::test]
    #[ignore = "requires VersConfig (/etc/vers) — run on SNE nodes only"]
    async fn test_reserve_network_skips_networks_bound_to_vms() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 4, 1);
        db.insert_vm_network(test_vm_network_record(host, Ipv4Addr::new(10, 0, 4, 2)))
            .await
            .unwrap();

        // Insert a VM that references this network
        let vm = test_vm_record(host);
        db.insert_vm_process_record(&VmProcessRecord {
            pid: vm.vm_process_pid,
            process_type: HypervisorType::Firecracker,
            vm_id: vm.id,
        })
        .await
        .unwrap();
        db.insert_vm_record(vm).await.unwrap();

        // Network is bound to a VM, so reserve should return None
        let reserved = db.reserve_network().await.unwrap();
        assert!(reserved.is_none());
    }

    #[tokio::test]
    async fn test_set_wg_on_vm_network() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 5, 1);
        db.insert_vm_network(test_vm_network_record(host, Ipv4Addr::new(10, 0, 5, 2)))
            .await
            .unwrap();

        let wg = VmWireGuardConfig {
            interface_name: "wg-test".to_string(),
            private_key: "privkey123".to_string(),
            private_ip: "fd00::1".parse().unwrap(),
            peer_pub_key: "pubkey456".to_string(),
            peer_pub_ip: "203.0.113.1".parse().unwrap(),
            peer_ipv6: "fd00::2".parse().unwrap(),
            wg_port: 51820,
        };

        let result = db.set_wg_on_vm_network(&host, Some(wg)).await.unwrap();
        assert!(result.is_some());

        // Verify WG config was set
        let net = db.fetch_vm_network(&host).await.unwrap().unwrap();
        assert!(net.wg.is_some());
        let fetched_wg = net.wg.unwrap();
        assert_eq!(fetched_wg.interface_name, "wg-test");
        assert_eq!(fetched_wg.wg_port, 51820);

        // Clear WG config
        db.set_wg_on_vm_network(&host, None).await.unwrap();
        let net2 = db.fetch_vm_network(&host).await.unwrap().unwrap();
        assert!(net2.wg.is_none());
    }

    #[tokio::test]
    async fn test_set_wg_on_nonexistent_network_returns_none() {
        let (db, _f) = make_db().await;
        let wg = VmWireGuardConfig {
            interface_name: "wg-test".to_string(),
            private_key: "privkey123".to_string(),
            private_ip: "fd00::1".parse().unwrap(),
            peer_pub_key: "pubkey456".to_string(),
            peer_pub_ip: "203.0.113.1".parse().unwrap(),
            peer_ipv6: "fd00::2".parse().unwrap(),
            wg_port: 51820,
        };

        let result = db
            .set_wg_on_vm_network(&Ipv4Addr::new(99, 99, 99, 99), Some(wg))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    // ── VM Record CRUD ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_insert_and_fetch_vm_record() {
        let (db, _f) = make_db().await;
        let vm = test_vm_record(Ipv4Addr::new(10, 0, 10, 1));
        let vm_id = vm.id;
        let vol_id = vm.vm_volume_id;

        db.insert_vm_record(vm).await.unwrap();

        let fetched = db.fetch_vm_record(&vm_id).await.unwrap();
        assert!(fetched.is_some());
        let f = fetched.unwrap();
        assert_eq!(f.id, vm_id);
        assert_eq!(f.vcpu_count, 1);
        assert_eq!(f.mem_size_mib, 512);
        assert_eq!(f.fs_size_mib, 1024);
        assert_eq!(f.vm_volume_id, vol_id);
        assert_eq!(f.kernel_name, "default.bin");
        assert_eq!(f.image_name, "default");
    }

    #[tokio::test]
    async fn test_fetch_vm_record_nonexistent_returns_none() {
        let (db, _f) = make_db().await;
        let result = db.fetch_vm_record(&Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_vm_record() {
        let (db, _f) = make_db().await;
        let vm = test_vm_record(Ipv4Addr::new(10, 0, 11, 1));
        let vm_id = vm.id;
        db.insert_vm_record(vm).await.unwrap();

        db.delete_vm_record(&vm_id).await.unwrap();
        assert!(db.fetch_vm_record(&vm_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_list_all_vm_ids() {
        let (db, _f) = make_db().await;

        // Empty at first
        assert!(db.list_all_vm_ids().await.unwrap().is_empty());

        let vm1 = test_vm_record(Ipv4Addr::new(10, 0, 12, 1));
        let vm2 = test_vm_record(Ipv4Addr::new(10, 0, 12, 2));
        let id1 = vm1.id;
        let id2 = vm2.id;
        db.insert_vm_record(vm1).await.unwrap();
        db.insert_vm_record(vm2).await.unwrap();

        let ids = db.list_all_vm_ids().await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[tokio::test]
    async fn test_list_all_vms_with_pids() {
        let (db, _f) = make_db().await;
        let mut vm = test_vm_record(Ipv4Addr::new(10, 0, 13, 1));
        vm.vm_process_pid = 42;
        let vm_id = vm.id;
        db.insert_vm_record(vm).await.unwrap();

        let vms = db.list_all_vms_with_pids().await.unwrap();
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0], (vm_id, 42));
    }

    #[tokio::test]
    async fn test_count_vms() {
        let (db, _f) = make_db().await;
        assert_eq!(db.count_vms().await.unwrap(), 0);

        db.insert_vm_record(test_vm_record(Ipv4Addr::new(10, 0, 14, 1)))
            .await
            .unwrap();
        db.insert_vm_record(test_vm_record(Ipv4Addr::new(10, 0, 14, 2)))
            .await
            .unwrap();
        assert_eq!(db.count_vms().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_update_vm_process_pid() {
        let (db, _f) = make_db().await;
        let vm = test_vm_record(Ipv4Addr::new(10, 0, 15, 1));
        let vm_id = vm.id;
        db.insert_vm_record(vm).await.unwrap();

        db.update_vm_process_pid(&vm_id, 7777).await.unwrap();
        let fetched = db.fetch_vm_record(&vm_id).await.unwrap().unwrap();
        assert_eq!(fetched.vm_process_pid, 7777);
    }

    #[tokio::test]
    async fn test_get_vm_vcpu_and_ram_usage() {
        let (db, _f) = make_db().await;

        // Empty DB should return (0, 0)
        let (vcpu, mem) = db.get_vm_vcpu_and_ram_usage().await.unwrap();
        assert_eq!(vcpu, 0);
        assert_eq!(mem, 0);

        let mut vm1 = test_vm_record(Ipv4Addr::new(10, 0, 16, 1));
        vm1.vcpu_count = 2;
        vm1.mem_size_mib = 1024;
        let mut vm2 = test_vm_record(Ipv4Addr::new(10, 0, 16, 2));
        vm2.vcpu_count = 4;
        vm2.mem_size_mib = 2048;

        db.insert_vm_record(vm1).await.unwrap();
        db.insert_vm_record(vm2).await.unwrap();

        let (vcpu, mem) = db.get_vm_vcpu_and_ram_usage().await.unwrap();
        assert_eq!(vcpu, 6);
        assert_eq!(mem, 3072);
    }

    #[tokio::test]
    async fn test_fetch_vm_with_network_joined() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 17, 1);
        db.insert_vm_network(test_vm_network_record(host, Ipv4Addr::new(10, 0, 17, 2)))
            .await
            .unwrap();

        let vm = test_vm_record(host);
        let vm_id = vm.id;
        db.insert_vm_record(vm).await.unwrap();

        let result = db.fetch_vm_with_network(&vm_id).await.unwrap();
        assert!(result.is_some());
        let (vm_rec, net_opt) = result.unwrap();
        assert_eq!(vm_rec.id, vm_id);
        assert!(net_opt.is_some());
        let net = net_opt.unwrap();
        assert_eq!(net.host_addr, host);
    }

    #[tokio::test]
    async fn test_fetch_vm_with_network_no_network() {
        let (db, _f) = make_db().await;
        // Insert VM pointing to a network that doesn't exist in vm_network table
        let vm = test_vm_record(Ipv4Addr::new(10, 0, 18, 1));
        let vm_id = vm.id;
        db.insert_vm_record(vm).await.unwrap();

        let result = db.fetch_vm_with_network(&vm_id).await.unwrap();
        assert!(result.is_some());
        let (vm_rec, net_opt) = result.unwrap();
        assert_eq!(vm_rec.id, vm_id);
        assert!(net_opt.is_none());
    }

    #[tokio::test]
    async fn test_fetch_vm_with_network_nonexistent_vm() {
        let (db, _f) = make_db().await;
        let result = db.fetch_vm_with_network(&Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    // ── VmProcess CRUD ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_vm_process_crud() {
        let (db, _f) = make_db().await;
        let vm_id = Uuid::new_v4();
        let rec = VmProcessRecord {
            pid: 5000,
            process_type: HypervisorType::Firecracker,
            vm_id,
        };

        // Insert
        db.insert_vm_process_record(&rec).await.unwrap();

        // Fetch
        let fetched = db.fetch_vm_process_record(5000).await.unwrap();
        assert!(fetched.is_some());
        let f = fetched.unwrap();
        assert_eq!(f.pid, 5000);
        assert_eq!(f.vm_id, vm_id);

        // Delete
        db.delete_vm_process_record(5000).await.unwrap();
        assert!(db.fetch_vm_process_record(5000).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_fetch_vm_process_nonexistent() {
        let (db, _f) = make_db().await;
        assert!(db.fetch_vm_process_record(99999).await.unwrap().is_none());
    }

    // ── CephVmVolume CRUD ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_ceph_volume_crud() {
        let (db, _f) = make_db().await;
        let vol_id = Uuid::new_v4();
        let rec = CephVmVolumeRecord {
            id: vol_id,
            image_name: "rbd/test-image".to_string(),
            device_path: "/dev/rbd0".to_string(),
        };

        // Insert
        db.insert_ceph_vm_volume_record(rec).await.unwrap();

        // Fetch
        let fetched = db.fetch_ceph_vm_volume_record(&vol_id).await.unwrap();
        assert!(fetched.is_some());
        let f = fetched.unwrap();
        assert_eq!(f.id, vol_id);
        assert_eq!(f.image_name, "rbd/test-image");
        assert_eq!(f.device_path, "/dev/rbd0");

        // Delete
        db.delete_ceph_vm_volume_record(&vol_id).await.unwrap();
        assert!(
            db.fetch_ceph_vm_volume_record(&vol_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_fetch_ceph_volume_nonexistent() {
        let (db, _f) = make_db().await;
        assert!(
            db.fetch_ceph_vm_volume_record(&Uuid::new_v4())
                .await
                .unwrap()
                .is_none()
        );
    }

    // ── VmReadyServiceStore ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_vm_exists() {
        let (db, _f) = make_db().await;
        let vm = test_vm_record(Ipv4Addr::new(10, 0, 20, 1));
        let vm_id = vm.id;

        assert!(!db.vm_exists(&vm_id).await.unwrap());
        db.insert_vm_record(vm).await.unwrap();
        assert!(db.vm_exists(&vm_id).await.unwrap());
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_all_vm_pids_empty() {
        let (db, _f) = make_db().await;
        let pids = db.list_all_vm_pids().await.unwrap();
        assert!(pids.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires VersConfig (/etc/vers) — run on SNE nodes only"]
    async fn test_reserve_network_empty_db() {
        let (db, _f) = make_db().await;
        let result = db.reserve_network().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_vm_is_noop() {
        let (db, _f) = make_db().await;
        // Should not error
        db.delete_vm_record(&Uuid::new_v4()).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_nonexistent_network_is_noop() {
        let (db, _f) = make_db().await;
        db.delete_vm_network(&Ipv4Addr::new(1, 1, 1, 1))
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires VersConfig (/etc/vers) — run on SNE nodes only"]
    async fn test_multiple_networks_reserve_picks_available() {
        let (db, _f) = make_db().await;

        // Insert two networks, both expired
        for i in 1..=2u8 {
            db.insert_vm_network(test_vm_network_record(
                Ipv4Addr::new(10, 0, 30, i),
                Ipv4Addr::new(10, 0, 30, i + 100),
            ))
            .await
            .unwrap();
        }

        // Reserve first
        let first = db.reserve_network().await.unwrap().unwrap();
        // Reserve second
        let second = db.reserve_network().await.unwrap().unwrap();
        // Third should be None
        assert!(db.reserve_network().await.unwrap().is_none());

        // The two should be different
        assert_ne!(first.host_addr, second.host_addr);
    }

    #[tokio::test]
    async fn test_usage_sums_after_insert_and_delete() {
        let (db, _f) = make_db().await;

        let mut vm = test_vm_record(Ipv4Addr::new(10, 0, 31, 1));
        vm.vcpu_count = 3;
        vm.mem_size_mib = 768;
        let vm_id = vm.id;
        db.insert_vm_record(vm).await.unwrap();

        let (vcpu, mem) = db.get_vm_vcpu_and_ram_usage().await.unwrap();
        assert_eq!(vcpu, 3);
        assert_eq!(mem, 768);

        db.delete_vm_record(&vm_id).await.unwrap();
        let (vcpu, mem) = db.get_vm_vcpu_and_ram_usage().await.unwrap();
        assert_eq!(vcpu, 0);
        assert_eq!(mem, 0);
    }

    #[tokio::test]
    async fn test_fetch_vm_with_network_includes_wg_config() {
        let (db, _f) = make_db().await;
        let host = Ipv4Addr::new(10, 0, 32, 1);

        let wg = VmWireGuardConfig {
            interface_name: "wg-joined".to_string(),
            private_key: "privkey".to_string(),
            private_ip: "fd00::10".parse().unwrap(),
            peer_pub_key: "peerpub".to_string(),
            peer_pub_ip: "198.51.100.1".parse().unwrap(),
            peer_ipv6: "fd00::20".parse().unwrap(),
            wg_port: 51821,
        };

        db.insert_vm_network(VmNetworkRecord {
            host_addr: host.to_bits(),
            vm_addr: Ipv4Addr::new(10, 0, 32, 2).to_bits(),
            netns_name: "ns-wg".to_string(),
            ssh_port: 28001,
            wg: Some(wg),
            reserved_until: (Utc::now() - Duration::seconds(60)).to_rfc3339(),
        })
        .await
        .unwrap();

        let vm = test_vm_record(host);
        let vm_id = vm.id;
        db.insert_vm_record(vm).await.unwrap();

        let (_, net_opt) = db.fetch_vm_with_network(&vm_id).await.unwrap().unwrap();
        let net = net_opt.unwrap();
        assert!(net.wg.is_some());
        let fetched_wg = net.wg.unwrap();
        assert_eq!(fetched_wg.interface_name, "wg-joined");
        assert_eq!(fetched_wg.wg_port, 51821);
    }
}
