use chrono::{DateTime, Utc};
use serde::Serialize;
use std::future::Future;
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait CommitRepositoriesRepository {
    fn insert(
        &self,
        name: String,
        org_id: Uuid,
        owner_id: Uuid,
        description: Option<String>,
    ) -> impl Future<Output = Result<CommitRepositoryEntity, DBError>>;

    fn get_by_name(
        &self,
        org_id: Uuid,
        name: &str,
    ) -> impl Future<Output = Result<Option<CommitRepositoryEntity>, DBError>>;

    fn get_by_id(
        &self,
        repo_id: Uuid,
    ) -> impl Future<Output = Result<Option<CommitRepositoryEntity>, DBError>>;

    fn list_by_org(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CommitRepositoryEntity>, DBError>>;

    fn delete(&self, repo_id: Uuid) -> impl Future<Output = Result<bool, DBError>>;

    fn set_public(
        &self,
        repo_id: Uuid,
        is_public: bool,
    ) -> impl Future<Output = Result<bool, DBError>>;

    fn list_public(&self) -> impl Future<Output = Result<Vec<CommitRepositoryEntity>, DBError>>;

    /// Look up a public repository by org name + repo name (for unauthenticated access).
    fn get_public_by_org_and_name(
        &self,
        org_name: &str,
        repo_name: &str,
    ) -> impl Future<Output = Result<Option<CommitRepositoryEntity>, DBError>>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitRepositoryEntity {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: Uuid,
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
}

impl From<Row> for CommitRepositoryEntity {
    fn from(row: Row) -> Self {
        Self {
            id: row.get("repo_id"),
            org_id: row.get("org_id"),
            name: row.get("name"),
            description: row.get("description"),
            owner_id: row.get("owner_id"),
            is_public: row.get("is_public"),
            created_at: row.get("created_at"),
        }
    }
}

pub struct CommitRepositories(DB);

impl DB {
    pub fn commit_repositories(&self) -> CommitRepositories {
        CommitRepositories(self.clone())
    }
}

impl CommitRepositoriesRepository for CommitRepositories {
    async fn insert(
        &self,
        name: String,
        org_id: Uuid,
        owner_id: Uuid,
        description: Option<String>,
    ) -> Result<CommitRepositoryEntity, DBError> {
        let repo_id = Uuid::new_v4();
        let created_at = Utc::now();
        let is_public = false;

        let rows = execute_sql!(
            self.0,
            "INSERT INTO commit_repositories (repo_id, org_id, name, description, owner_id, is_public, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::TEXT,
                Type::UUID,
                Type::BOOL,
                Type::TIMESTAMPTZ,
            ],
            &[&repo_id, &org_id, &name, &description, &owner_id, &is_public, &created_at]
        )?;
        debug_assert!(rows == 1);

        Ok(CommitRepositoryEntity {
            id: repo_id,
            org_id,
            name,
            description,
            owner_id,
            is_public,
            created_at,
        })
    }

    async fn get_by_name(
        &self,
        org_id: Uuid,
        name: &str,
    ) -> Result<Option<CommitRepositoryEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM commit_repositories WHERE org_id = $1 AND name = $2",
            &[Type::UUID, Type::TEXT],
            &[&org_id, &name]
        )?;
        Ok(row.map(|r| r.into()))
    }

    async fn get_by_id(&self, repo_id: Uuid) -> Result<Option<CommitRepositoryEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM commit_repositories WHERE repo_id = $1",
            &[Type::UUID],
            &[&repo_id]
        )?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<CommitRepositoryEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM commit_repositories WHERE org_id = $1 ORDER BY name ASC",
            &[Type::UUID],
            &[&org_id]
        )?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn delete(&self, repo_id: Uuid) -> Result<bool, DBError> {
        let rows = execute_sql!(
            self.0,
            "DELETE FROM commit_repositories WHERE repo_id = $1",
            &[Type::UUID],
            &[&repo_id]
        )?;
        Ok(rows == 1)
    }

    async fn set_public(&self, repo_id: Uuid, is_public: bool) -> Result<bool, DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE commit_repositories SET is_public = $2 WHERE repo_id = $1",
            &[Type::UUID, Type::BOOL],
            &[&repo_id, &is_public]
        )?;
        Ok(rows == 1)
    }

    async fn list_public(&self) -> Result<Vec<CommitRepositoryEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT cr.* FROM commit_repositories cr
             WHERE cr.is_public = TRUE
             ORDER BY cr.name ASC",
            &[],
            &[]
        )?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn get_public_by_org_and_name(
        &self,
        org_name: &str,
        repo_name: &str,
    ) -> Result<Option<CommitRepositoryEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT cr.* FROM commit_repositories cr
             JOIN organizations o ON o.org_id = cr.org_id
             WHERE o.name = $1 AND cr.name = $2 AND cr.is_public = TRUE",
            &[Type::TEXT, Type::TEXT],
            &[&org_name, &repo_name]
        )?;
        Ok(row.map(|r| r.into()))
    }
}
