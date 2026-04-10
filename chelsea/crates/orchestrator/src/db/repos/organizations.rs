use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

/// Current resource usage for an org (active VMs).
#[derive(Debug, Clone, Default)]
pub struct OrgResourceUsage {
    pub vcpus: i64,
    pub memory_mib: i64,
}

pub trait OrgsRepository {
    fn get_by_id(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Option<OrganizationEntity>, DBError>>;

    fn get_by_name(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<OrganizationEntity>, DBError>>;

    /// Sum vCPUs and memory across all active (running) VMs owned by this org.
    fn resource_usage(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<OrgResourceUsage, DBError>>;

    /// Update user-configurable resource limits.
    /// Returns an error if the requested limits exceed the admin ceiling
    /// (enforced by a CHECK constraint in the DB).
    fn update_resource_limits(
        &self,
        org_id: Uuid,
        max_vcpus: i32,
        max_memory_mib: i64,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Admin-only: update the admin ceiling and optionally the user limits.
    /// If user limits currently exceed the new ceiling, they are clamped down.
    fn update_admin_limits(
        &self,
        org_id: Uuid,
        admin_max_vcpus: i32,
        admin_max_memory_mib: i64,
    ) -> impl Future<Output = Result<(), DBError>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationEntity {
    org_id: Uuid,
    parent_org_id: Option<Uuid>,
    account_id: Uuid,
    billing_contact_id: Uuid,

    name: String,
    description: Option<String>,
    avatar_uri: Option<String>,

    created_at: DateTime<Utc>,

    /// Admin ceiling: hard platform limit, only changeable by admins.
    admin_max_vcpus: i32,
    /// Admin ceiling: hard platform limit, only changeable by admins.
    admin_max_memory_mib: i64,
    /// User-configurable limit (must be ≤ admin ceiling). VM creation checks this.
    max_vcpus: i32,
    /// User-configurable limit (must be ≤ admin ceiling). VM creation checks this.
    max_memory_mib: i64,
}

impl OrganizationEntity {
    pub fn id(&self) -> Uuid {
        self.org_id
    }
    pub fn account_id(&self) -> Uuid {
        self.account_id
    }
    pub fn billing_contact_id(&self) -> Uuid {
        self.billing_contact_id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Admin-set hard ceiling for vCPUs.
    pub fn admin_max_vcpus(&self) -> i32 {
        self.admin_max_vcpus
    }
    /// Admin-set hard ceiling for memory.
    pub fn admin_max_memory_mib(&self) -> i64 {
        self.admin_max_memory_mib
    }
    /// User-configurable vCPU limit (≤ admin ceiling). VM creation checks this.
    pub fn max_vcpus(&self) -> i32 {
        self.max_vcpus
    }
    /// User-configurable memory limit (≤ admin ceiling). VM creation checks this.
    pub fn max_memory_mib(&self) -> i64 {
        self.max_memory_mib
    }
}

impl From<Row> for OrganizationEntity {
    fn from(row: Row) -> Self {
        Self {
            org_id: row.get("org_id"),
            account_id: row.get("account_id"),
            parent_org_id: row.get("parent_org_id"),
            billing_contact_id: row.get("billing_contact_id"),
            name: row.get("name"),
            description: row.get("description"),
            avatar_uri: row.get("avatar_uri"),
            created_at: row.get("created_at"),
            admin_max_vcpus: row.get("admin_max_vcpus"),
            admin_max_memory_mib: row.get("admin_max_memory_mib"),
            max_vcpus: row.get("max_vcpus"),
            max_memory_mib: row.get("max_memory_mib"),
        }
    }
}

pub struct Organizations(DB);

impl DB {
    pub fn orgs(&self) -> Organizations {
        Organizations(self.clone())
    }
}

impl OrgsRepository for Organizations {
    async fn get_by_id(&self, org_id: Uuid) -> Result<Option<OrganizationEntity>, DBError> {
        let tes = query_one_sql!(
            self.0,
            "SELECT * FROM organizations WHERE org_id = $1",
            &[Type::UUID],
            &[&org_id]
        )?;

        Ok(tes.map(|row| row.into()))
    }

    async fn get_by_name(&self, name: &str) -> Result<Option<OrganizationEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM organizations WHERE name = $1",
            &[Type::TEXT],
            &[&name]
        )?;

        Ok(row.map(|row| row.into()))
    }

    async fn update_resource_limits(
        &self,
        org_id: Uuid,
        max_vcpus: i32,
        max_memory_mib: i64,
    ) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "UPDATE organizations SET max_vcpus = $1, max_memory_mib = $2 WHERE org_id = $3",
            &[Type::INT4, Type::INT8, Type::UUID],
            &[&max_vcpus, &max_memory_mib, &org_id]
        )?;
        Ok(())
    }

    async fn update_admin_limits(
        &self,
        org_id: Uuid,
        admin_max_vcpus: i32,
        admin_max_memory_mib: i64,
    ) -> Result<(), DBError> {
        // Update admin ceiling and clamp user limits down if they exceed the new ceiling.
        execute_sql!(
            self.0,
            "UPDATE organizations SET
                admin_max_vcpus = $1,
                admin_max_memory_mib = $2,
                max_vcpus = LEAST(max_vcpus, $1),
                max_memory_mib = LEAST(max_memory_mib, $2)
             WHERE org_id = $3",
            &[Type::INT4, Type::INT8, Type::UUID],
            &[&admin_max_vcpus, &admin_max_memory_mib, &org_id]
        )?;
        Ok(())
    }

    async fn resource_usage(&self, org_id: Uuid) -> Result<OrgResourceUsage, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT
                COALESCE(SUM(vms.vcpu_count::bigint), 0)::bigint AS total_vcpus,
                COALESCE(SUM(vms.mem_size_mib::bigint), 0)::bigint AS total_memory_mib
            FROM vms
            JOIN api_keys ON vms.owner_id = api_keys.api_key_id
            WHERE api_keys.org_id = $1
              AND vms.deleted_at IS NULL
              AND vms.node_id IS NOT NULL",
            &[Type::UUID],
            &[&org_id]
        )?;

        match row {
            Some(r) => Ok(OrgResourceUsage {
                vcpus: r.get("total_vcpus"),
                memory_mib: r.get("total_memory_mib"),
            }),
            None => Ok(OrgResourceUsage::default()),
        }
    }
}
