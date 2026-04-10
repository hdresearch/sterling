use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::{DB, DBError};

use super::{
    organizations::{OrgResourceUsage, OrganizationEntity},
    vms::VmEntity,
};

/// Parameters for inserting a VM row.
pub struct VmInsertParams {
    pub vm_id: Uuid,
    pub parent_commit_id: Option<Uuid>,
    pub grandparent_vm_id: Option<Uuid>,
    pub node_id: Uuid,
    pub ip: Ipv6Addr,
    pub wg_private_key: String,
    pub wg_public_key: String,
    pub wg_port: u16,
    pub owner_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub vcpu_count: i32,
    pub mem_size_mib: i32,
    pub labels: Option<HashMap<String, String>>,
}

/// Error from the atomic check-and-insert operation.
#[derive(Debug, thiserror::Error)]
pub enum CheckAndInsertError {
    #[error("{0}")]
    Db(#[from] DBError),
    #[error("{0}")]
    ResourceLimit(#[from] crate::action::vms::ResourceLimitError),
    #[error("not unique node_id/wg_port combination")]
    NotUniqueNodeIdWgPortCombination,
}

impl DB {
    /// Atomically check resource limits and insert a VM row.
    ///
    /// Uses a Postgres advisory lock on the org ID to serialize concurrent
    /// VM creation for the same org. Within the lock:
    /// 1. Sums current resource usage from `vms` (non-deleted rows)
    /// 2. Checks against org limits
    /// 3. Inserts the new VM row
    ///
    /// The advisory lock is transaction-scoped (`pg_advisory_xact_lock`) and
    /// released automatically when the transaction commits or rolls back.
    pub async fn check_limits_and_insert_vm(
        &self,
        org: &OrganizationEntity,
        params: VmInsertParams,
    ) -> Result<VmEntity, CheckAndInsertError> {
        let conn = self.raw_obj().await;

        // Begin transaction
        conn.simple_query("BEGIN").await?;

        // Acquire advisory lock keyed on the org ID.
        // We use the upper 64 bits of the UUID as the lock key.
        let org_id = org.id();
        let lock_key = uuid_to_advisory_key(org_id);
        conn.execute("SELECT pg_advisory_xact_lock($1)", &[&lock_key])
            .await?;

        // Read current resource usage under the lock.
        let usage_row = conn
            .query_one(
                "SELECT
                    COALESCE(SUM(vms.vcpu_count::bigint), 0)::bigint AS total_vcpus,
                    COALESCE(SUM(vms.mem_size_mib::bigint), 0)::bigint AS total_memory_mib
                FROM vms
                JOIN api_keys ON vms.owner_id = api_keys.api_key_id
                WHERE api_keys.org_id = $1
                  AND vms.deleted_at IS NULL
                  AND vms.node_id IS NOT NULL",
                &[&org_id],
            )
            .await?;

        let usage = OrgResourceUsage {
            vcpus: usage_row.get("total_vcpus"),
            memory_mib: usage_row.get("total_memory_mib"),
        };

        // Check vCPU limit
        let new_vcpus = usage.vcpus + params.vcpu_count as i64;
        if new_vcpus > org.max_vcpus() as i64 {
            conn.simple_query("ROLLBACK").await?;
            return Err(crate::action::vms::ResourceLimitError {
                message: format!(
                    "vCPU limit exceeded: this VM requires {} vCPUs, but org is using {}/{} vCPUs",
                    params.vcpu_count,
                    usage.vcpus,
                    org.max_vcpus()
                ),
            }
            .into());
        }

        // Check memory limit
        let new_memory = usage.memory_mib + params.mem_size_mib as i64;
        if new_memory > org.max_memory_mib() {
            conn.simple_query("ROLLBACK").await?;
            return Err(crate::action::vms::ResourceLimitError {
                message: format!(
                    "Memory limit exceeded: this VM requires {} MiB, but org is using {}/{} MiB",
                    params.mem_size_mib,
                    usage.memory_mib,
                    org.max_memory_mib()
                ),
            }
            .into());
        }

        // Insert the VM row — this "claims" the resources.
        let insert_result = conn
            .execute(
                "INSERT INTO vms (vm_id, parent_commit_id, grandparent_vm_id, node_id, ip, wg_private_key, wg_public_key, wg_port, owner_id, created_at, deleted_at, vcpu_count, mem_size_mib)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
                &[
                    &params.vm_id,
                    &params.parent_commit_id,
                    &params.grandparent_vm_id,
                    &params.node_id,
                    &IpAddr::V6(params.ip),
                    &params.wg_private_key,
                    &params.wg_public_key,
                    &(params.wg_port as i32),
                    &params.owner_id,
                    &params.created_at,
                    &params.deleted_at,
                    &params.vcpu_count,
                    &params.mem_size_mib,
                ],
            )
            .await;

        for (k, v) in params.labels.clone().unwrap_or_default() {
            conn.execute(
                "INSERT INTO labels (vm_id, label_name, label_value)
                         VALUES ($1, $2, $3)",
                &[&params.vm_id, &k, &v],
            )
            .await?;
        }

        match insert_result {
            Ok(_) => {
                conn.simple_query("COMMIT").await?;
                Ok(VmEntity {
                    vm_id: params.vm_id,
                    parent_commit_id: params.parent_commit_id,
                    grandparent_vm_id: params.grandparent_vm_id,
                    node_id: Some(params.node_id),
                    ip: params.ip,
                    wg_private_key: params.wg_private_key,
                    wg_public_key: params.wg_public_key,
                    wg_port: params.wg_port,
                    owner_id: params.owner_id,
                    created_at: params.created_at,
                    deleted_at: params.deleted_at,
                    labels: params.labels,
                })
            }
            Err(err) => {
                conn.simple_query("ROLLBACK").await?;
                match err.as_db_error() {
                    Some(db_err)
                        if db_err
                            .constraint()
                            .is_some_and(|c| c == "wg_port_node_id_pair_unique") =>
                    {
                        Err(CheckAndInsertError::NotUniqueNodeIdWgPortCombination)
                    }
                    _ => Err(err.into()),
                }
            }
        }
    }
}

/// Convert a UUID to a deterministic i64 advisory lock key.
/// Uses the first 8 bytes of the UUID.
fn uuid_to_advisory_key(id: Uuid) -> i64 {
    let bytes = id.as_bytes();
    i64::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}
