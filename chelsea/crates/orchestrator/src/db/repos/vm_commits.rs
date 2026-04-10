use chrono::{DateTime, Utc};
use serde::Serialize;
use std::future::Future;
use tokio_postgres::{Row, types::Type};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait VMCommitsRepository {
    fn insert(
        &self,
        commit_id: Uuid,
        parent_vm_id: Option<Uuid>,
        grandparent_commit_id: Option<Uuid>,
        owner_id: Uuid,
        name: String,
        description: Option<String>,
        created_at: DateTime<Utc>,
        is_public: bool,
    ) -> impl Future<Output = Result<VmCommitEntity, DBError>>;

    fn list_by_vm(&self, vm_id: Uuid)
    -> impl Future<Output = Result<Vec<VmCommitEntity>, DBError>>;

    fn get_latest_by_vm(
        &self,
        vm_id: Uuid,
    ) -> impl Future<Output = Result<Option<VmCommitEntity>, DBError>>;

    fn get_by_id(
        &self,
        commit_id: Uuid,
    ) -> impl Future<Output = Result<Option<VmCommitEntity>, DBError>>;

    fn list_by_cluster(
        &self,
        cluster_id: Uuid,
    ) -> impl Future<Output = Result<Vec<VmCommitEntity>, DBError>>;

    fn list_by_owner(
        &self,
        owner_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = Result<Vec<VmCommitEntity>, DBError>>;

    fn count_by_owner(&self, owner_id: Uuid) -> impl Future<Output = Result<i64, DBError>>;
    fn get_parent_commits_recursive(
        &self,
        commit_id: Uuid,
    ) -> impl Future<Output = Result<Vec<VmCommitEntity>, DBError>>;

    fn mark_deleted(
        &self,
        commit_id: Uuid,
        deleted_by: Uuid,
        deleted_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, DBError>>;

    /// Hard delete a commit record. Use with caution - typically you want mark_deleted() instead.
    /// This is only for rollback scenarios where a commit was never actually completed.
    fn hard_delete(&self, commit_id: Uuid) -> impl Future<Output = Result<bool, DBError>>;

    fn clear_deleted(&self, commit_id: Uuid) -> impl Future<Output = Result<(), DBError>>;

    fn set_public(
        &self,
        commit_id: Uuid,
        is_public: bool,
    ) -> impl Future<Output = Result<bool, DBError>>;

    fn update_metadata(
        &self,
        commit_id: Uuid,
        is_public: bool,
        name: Option<String>,
        description: Option<String>,
    ) -> impl Future<Output = Result<bool, DBError>>;

    fn list_public(
        &self,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = Result<Vec<VmCommitEntity>, DBError>>;

    fn count_public(&self) -> impl Future<Output = Result<i64, DBError>>;
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VmCommitEntity {
    pub id: Uuid,
    /// The VM that this commit was created from, if any.
    pub parent_vm_id: Option<Uuid>,
    /// The commit that this commit's parent VM was started from, if any. Intended to optimize traversing the commit tree.
    pub grandparent_commit_id: Option<Uuid>,
    /// api key id.
    pub owner_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Whether this commit is publicly accessible (readable/restorable by anyone).
    pub is_public: bool,
}

impl From<Row> for VmCommitEntity {
    fn from(row: Row) -> Self {
        Self {
            id: row.get("commit_id"),
            parent_vm_id: row.get("parent_vm_id"),
            grandparent_commit_id: row.get("grandparent_commit_id"),
            owner_id: row.get("owner_id"),
            name: row.get("name"),
            description: row.get("description"),
            created_at: row.get("created_at"),
            is_public: row.get("is_public"),
        }
    }
}

pub struct VMCommits(DB);

impl DB {
    pub fn commits(&self) -> VMCommits {
        VMCommits(self.clone())
    }
}

impl VMCommitsRepository for VMCommits {
    async fn insert(
        &self,
        commit_id: Uuid,
        parent_vm_id: Option<Uuid>,
        grandparent_commit_id: Option<Uuid>,
        owner_id: Uuid,
        name: String,
        description: Option<String>,
        created_at: DateTime<Utc>,
        is_public: bool,
    ) -> Result<VmCommitEntity, DBError> {
        let rows = execute_sql!(
            self.0,
            "INSERT INTO commits (
                commit_id, parent_vm_id, grandparent_commit_id, owner_id, name, description, created_at, is_public
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::TEXT,
                Type::TIMESTAMPTZ,
                Type::BOOL,
            ],
            &[
                &commit_id,
                &parent_vm_id,
                &grandparent_commit_id,
                &owner_id,
                &name,
                &description,
                &created_at,
                &is_public
            ]
        )?;
        debug_assert!(rows == 1);
        Ok(VmCommitEntity {
            id: commit_id,
            parent_vm_id,
            grandparent_commit_id,
            owner_id,
            name,
            description,
            created_at,
            is_public,
        })
    }

    async fn get_by_id(&self, commit_id: Uuid) -> Result<Option<VmCommitEntity>, DBError> {
        let rows = query_one_sql!(
            self.0,
            "SELECT * FROM commits WHERE commit_id = $1 AND deleted_at IS NULL",
            &[Type::UUID],
            &[&commit_id]
        )?;
        Ok(rows.map(|r| r.into()))
    }

    async fn list_by_vm(&self, vm_id: Uuid) -> Result<Vec<VmCommitEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM commits WHERE parent_vm_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
            &[Type::UUID],
            &[&vm_id]
        )?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn get_latest_by_vm(&self, vm_id: Uuid) -> Result<Option<VmCommitEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM commits WHERE parent_vm_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC LIMIT 1",
            &[Type::UUID],
            &[&vm_id]
        )?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_by_cluster(&self, cluster_id: Uuid) -> Result<Vec<VmCommitEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT commits.* FROM commits JOIN vms ON commits.parent_vm_id = vms.vm_id WHERE vms.cluster_id = $1 AND commits.deleted_at IS NULL",
            &[Type::UUID],
            &[&cluster_id]
        )?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn list_by_owner(
        &self,
        owner_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<VmCommitEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM commits WHERE owner_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            &[Type::UUID, Type::INT8, Type::INT8],
            &[&owner_id, &limit, &offset]
        )?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
    async fn get_parent_commits_recursive(
        &self,
        commit_id: Uuid,
    ) -> Result<Vec<VmCommitEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "WITH RECURSIVE commit_chain AS (
                SELECT * FROM commits WHERE commit_id = $1 AND deleted_at IS NULL
                UNION ALL
                SELECT c.*
                FROM commits c
                INNER JOIN commit_chain cc ON c.commit_id = cc.grandparent_commit_id
                WHERE c.deleted_at IS NULL
            )
            SELECT * FROM commit_chain",
            &[Type::UUID],
            &[&commit_id]
        )?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn count_by_owner(&self, owner_id: Uuid) -> Result<i64, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT COUNT(*) as count FROM commits WHERE owner_id = $1 AND deleted_at IS NULL",
            &[Type::UUID],
            &[&owner_id]
        )?;

        Ok(row.map(|r| r.get("count")).unwrap_or(0))
    }

    async fn mark_deleted(
        &self,
        commit_id: Uuid,
        deleted_by: Uuid,
        deleted_at: DateTime<Utc>,
    ) -> Result<bool, DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE commits SET deleted_at = $2, deleted_by = $3 WHERE commit_id = $1 AND deleted_at IS NULL",
            &[Type::UUID, Type::TIMESTAMPTZ, Type::UUID],
            &[&commit_id, &deleted_at, &deleted_by]
        )?;
        Ok(rows == 1)
    }

    async fn hard_delete(&self, commit_id: Uuid) -> Result<bool, DBError> {
        let rows = execute_sql!(
            self.0,
            "DELETE FROM commits WHERE commit_id = $1",
            &[Type::UUID],
            &[&commit_id]
        )?;
        Ok(rows == 1)
    }

    async fn clear_deleted(&self, commit_id: Uuid) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "UPDATE commits SET deleted_at = NULL, deleted_by = NULL WHERE commit_id = $1",
            &[Type::UUID],
            &[&commit_id]
        )?;
        Ok(())
    }

    async fn set_public(&self, commit_id: Uuid, is_public: bool) -> Result<bool, DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE commits SET is_public = $2 WHERE commit_id = $1",
            &[Type::UUID, Type::BOOL],
            &[&commit_id, &is_public]
        )?;
        Ok(rows > 0)
    }

    async fn update_metadata(
        &self,
        commit_id: Uuid,
        is_public: bool,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<bool, DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE commits SET is_public = $2, name = COALESCE($3, name), description = COALESCE($4, description) WHERE commit_id = $1",
            &[Type::UUID, Type::BOOL, Type::TEXT, Type::TEXT],
            &[&commit_id, &is_public, &name, &description]
        )?;
        Ok(rows > 0)
    }

    async fn list_public(&self, limit: i64, offset: i64) -> Result<Vec<VmCommitEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM commits WHERE is_public = TRUE ORDER BY created_at DESC LIMIT $1 OFFSET $2",
            &[Type::INT8, Type::INT8],
            &[&limit, &offset]
        )?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn count_public(&self) -> Result<i64, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT COUNT(*) as count FROM commits WHERE is_public = TRUE",
            &[],
            &[]
        )?;

        Ok(row.map(|r| r.get("count")).unwrap_or(0))
    }
}
