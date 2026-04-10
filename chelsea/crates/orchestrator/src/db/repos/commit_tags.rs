use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait CommitTagsRepository {
    fn insert(
        &self,
        tag_name: String,
        commit_id: Uuid,
        owner_id: Uuid,
        org_id: Uuid,
        description: Option<String>,
    ) -> impl Future<Output = Result<CommitTagEntity, DBError>>;

    /// Insert a tag scoped to a repository.
    fn insert_with_repo(
        &self,
        tag_name: String,
        commit_id: Uuid,
        owner_id: Uuid,
        org_id: Uuid,
        repo_id: Uuid,
        description: Option<String>,
    ) -> impl Future<Output = Result<CommitTagEntity, DBError>>;

    fn get_by_name(
        &self,
        org_id: Uuid,
        tag_name: &str,
    ) -> impl Future<Output = Result<Option<CommitTagEntity>, DBError>>;

    /// Get a tag by name within a specific repository.
    fn get_by_repo_and_name(
        &self,
        repo_id: Uuid,
        tag_name: &str,
    ) -> impl Future<Output = Result<Option<CommitTagEntity>, DBError>>;

    fn get_by_id(
        &self,
        tag_id: Uuid,
    ) -> impl Future<Output = Result<Option<CommitTagEntity>, DBError>>;

    fn list_by_org(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CommitTagEntity>, DBError>>;

    /// List all tags in a specific repository.
    fn list_by_repo(
        &self,
        repo_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CommitTagEntity>, DBError>>;

    fn list_by_commit(
        &self,
        commit_id: Uuid,
    ) -> impl Future<Output = Result<Vec<CommitTagEntity>, DBError>>;

    fn update_commit(
        &self,
        tag_id: Uuid,
        new_commit_id: Uuid,
    ) -> impl Future<Output = Result<CommitTagEntity, DBError>>;

    fn update_description(
        &self,
        tag_id: Uuid,
        description: Option<String>,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Atomically update both commit_id and/or description in a single query.
    fn update(
        &self,
        tag_id: Uuid,
        new_commit_id: Option<Uuid>,
        new_description: Option<Option<String>>,
    ) -> impl Future<Output = Result<CommitTagEntity, DBError>>;

    fn delete(&self, tag_id: Uuid) -> impl Future<Output = Result<(), DBError>>;

    /// Resolve a repo_name:tag_name reference to a commit_id within an organization.
    fn resolve_ref(
        &self,
        org_id: Uuid,
        repo_name: &str,
        tag_name: &str,
    ) -> impl Future<Output = Result<Option<CommitTagEntity>, DBError>>;

    /// Resolve a public repo reference: org_name/repo_name:tag_name.
    /// Only works for repos where is_public = TRUE.
    fn resolve_public_ref(
        &self,
        org_name: &str,
        repo_name: &str,
        tag_name: &str,
    ) -> impl Future<Output = Result<Option<CommitTagEntity>, DBError>>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitTagEntity {
    pub id: Uuid,
    pub tag_name: String,
    pub commit_id: Uuid,
    /// API key id that created this tag.
    pub owner_id: Uuid,
    /// Organization id that owns this tag.
    pub org_id: Uuid,
    /// Optional repository this tag belongs to. None for legacy org-scoped tags.
    pub repo_id: Option<Uuid>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Row> for CommitTagEntity {
    fn from(row: Row) -> Self {
        Self {
            id: row.get("tag_id"),
            tag_name: row.get("tag_name"),
            commit_id: row.get("commit_id"),
            owner_id: row.get("owner_id"),
            org_id: row.get("org_id"),
            repo_id: row.get("repo_id"),
            description: row.get("description"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }
}

pub struct CommitTags(DB);

impl DB {
    pub fn commit_tags(&self) -> CommitTags {
        CommitTags(self.clone())
    }
}

impl CommitTagsRepository for CommitTags {
    async fn insert(
        &self,
        tag_name: String,
        commit_id: Uuid,
        owner_id: Uuid,
        org_id: Uuid,
        description: Option<String>,
    ) -> Result<CommitTagEntity, DBError> {
        let tag_id = Uuid::new_v4();
        let created_at = Utc::now();
        let updated_at = created_at;

        let rows = execute_sql!(
            self.0,
            "INSERT INTO commit_tags (
                tag_id, tag_name, commit_id, owner_id, org_id, description, created_at, updated_at
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                Type::UUID,
                Type::TEXT,
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::TIMESTAMPTZ,
                Type::TIMESTAMPTZ,
            ],
            &[
                &tag_id,
                &tag_name,
                &commit_id,
                &owner_id,
                &org_id,
                &description,
                &created_at,
                &updated_at,
            ]
        )?;
        debug_assert!(rows == 1);
        Ok(CommitTagEntity {
            id: tag_id,
            tag_name,
            commit_id,
            owner_id,
            org_id,
            repo_id: None,
            description,
            created_at,
            updated_at,
        })
    }

    async fn insert_with_repo(
        &self,
        tag_name: String,
        commit_id: Uuid,
        owner_id: Uuid,
        org_id: Uuid,
        repo_id: Uuid,
        description: Option<String>,
    ) -> Result<CommitTagEntity, DBError> {
        let tag_id = Uuid::new_v4();
        let created_at = Utc::now();
        let updated_at = created_at;

        let rows = execute_sql!(
            self.0,
            "INSERT INTO commit_tags (
                tag_id, tag_name, commit_id, owner_id, org_id, repo_id, description, created_at, updated_at
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            &[
                Type::UUID,
                Type::TEXT,
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::TIMESTAMPTZ,
                Type::TIMESTAMPTZ,
            ],
            &[
                &tag_id,
                &tag_name,
                &commit_id,
                &owner_id,
                &org_id,
                &repo_id,
                &description,
                &created_at,
                &updated_at,
            ]
        )?;
        debug_assert!(rows == 1);
        Ok(CommitTagEntity {
            id: tag_id,
            tag_name,
            commit_id,
            owner_id,
            org_id,
            repo_id: Some(repo_id),
            description,
            created_at,
            updated_at,
        })
    }

    async fn get_by_name(
        &self,
        org_id: Uuid,
        tag_name: &str,
    ) -> Result<Option<CommitTagEntity>, DBError> {
        let rows = query_one_sql!(
            self.0,
            "SELECT * FROM commit_tags WHERE org_id = $1 AND tag_name = $2",
            &[Type::UUID, Type::TEXT],
            &[&org_id, &tag_name]
        )?;
        Ok(rows.map(|r| r.into()))
    }

    async fn get_by_id(&self, tag_id: Uuid) -> Result<Option<CommitTagEntity>, DBError> {
        let rows = query_one_sql!(
            self.0,
            "SELECT * FROM commit_tags WHERE tag_id = $1",
            &[Type::UUID],
            &[&tag_id]
        )?;
        Ok(rows.map(|r| r.into()))
    }

    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<CommitTagEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM commit_tags WHERE org_id = $1 ORDER BY tag_name ASC",
            &[Type::UUID],
            &[&org_id]
        )?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn list_by_commit(&self, commit_id: Uuid) -> Result<Vec<CommitTagEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM commit_tags WHERE commit_id = $1 ORDER BY tag_name ASC",
            &[Type::UUID],
            &[&commit_id]
        )?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update_commit(
        &self,
        tag_id: Uuid,
        new_commit_id: Uuid,
    ) -> Result<CommitTagEntity, DBError> {
        let updated_at = Utc::now();
        let row = query_one_sql!(
            self.0,
            "UPDATE commit_tags SET commit_id = $1, updated_at = $2 WHERE tag_id = $3 RETURNING *",
            &[Type::UUID, Type::TIMESTAMPTZ, Type::UUID],
            &[&new_commit_id, &updated_at, &tag_id]
        )?;

        Ok(row.expect("Tag must exist after successful update").into())
    }

    async fn update_description(
        &self,
        tag_id: Uuid,
        description: Option<String>,
    ) -> Result<(), DBError> {
        let updated_at = Utc::now();
        let rows = execute_sql!(
            self.0,
            "UPDATE commit_tags SET description = $1, updated_at = $2 WHERE tag_id = $3",
            &[Type::TEXT, Type::TIMESTAMPTZ, Type::UUID],
            &[&description, &updated_at, &tag_id]
        )?;
        debug_assert!(rows == 1);
        Ok(())
    }

    async fn update(
        &self,
        tag_id: Uuid,
        new_commit_id: Option<Uuid>,
        new_description: Option<Option<String>>,
    ) -> Result<CommitTagEntity, DBError> {
        let updated_at = Utc::now();

        // Build SET clause dynamically based on which fields are being updated
        let row = match (new_commit_id, new_description) {
            (Some(commit_id), Some(description)) => query_one_sql!(
                self.0,
                "UPDATE commit_tags SET commit_id = $1, description = $2, updated_at = $3 WHERE tag_id = $4 RETURNING *",
                &[Type::UUID, Type::TEXT, Type::TIMESTAMPTZ, Type::UUID],
                &[&commit_id, &description, &updated_at, &tag_id]
            )?,
            (Some(commit_id), None) => query_one_sql!(
                self.0,
                "UPDATE commit_tags SET commit_id = $1, updated_at = $2 WHERE tag_id = $3 RETURNING *",
                &[Type::UUID, Type::TIMESTAMPTZ, Type::UUID],
                &[&commit_id, &updated_at, &tag_id]
            )?,
            (None, Some(description)) => query_one_sql!(
                self.0,
                "UPDATE commit_tags SET description = $1, updated_at = $2 WHERE tag_id = $3 RETURNING *",
                &[Type::TEXT, Type::TIMESTAMPTZ, Type::UUID],
                &[&description, &updated_at, &tag_id]
            )?,
            (None, None) => {
                // No updates — just fetch current state
                return self
                    .get_by_id(tag_id)
                    .await
                    .map(|opt| opt.expect("Tag must exist when called with valid tag_id"));
            }
        };

        Ok(row.expect("Tag must exist after successful update").into())
    }

    async fn delete(&self, tag_id: Uuid) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "DELETE FROM commit_tags WHERE tag_id = $1",
            &[Type::UUID],
            &[&tag_id]
        )?;
        debug_assert!(rows <= 1);
        Ok(())
    }

    async fn get_by_repo_and_name(
        &self,
        repo_id: Uuid,
        tag_name: &str,
    ) -> Result<Option<CommitTagEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM commit_tags WHERE repo_id = $1 AND tag_name = $2",
            &[Type::UUID, Type::TEXT],
            &[&repo_id, &tag_name]
        )?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_by_repo(&self, repo_id: Uuid) -> Result<Vec<CommitTagEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM commit_tags WHERE repo_id = $1 ORDER BY tag_name ASC",
            &[Type::UUID],
            &[&repo_id]
        )?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn resolve_ref(
        &self,
        org_id: Uuid,
        repo_name: &str,
        tag_name: &str,
    ) -> Result<Option<CommitTagEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT ct.* FROM commit_tags ct
             JOIN commit_repositories cr ON ct.repo_id = cr.repo_id
             WHERE cr.org_id = $1 AND cr.name = $2 AND ct.tag_name = $3",
            &[Type::UUID, Type::TEXT, Type::TEXT],
            &[&org_id, &repo_name, &tag_name]
        )?;
        Ok(row.map(|r| r.into()))
    }

    async fn resolve_public_ref(
        &self,
        org_name: &str,
        repo_name: &str,
        tag_name: &str,
    ) -> Result<Option<CommitTagEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT ct.* FROM commit_tags ct
             JOIN commit_repositories cr ON ct.repo_id = cr.repo_id
             JOIN organizations o ON o.org_id = cr.org_id
             WHERE o.name = $1 AND cr.name = $2 AND ct.tag_name = $3 AND cr.is_public = TRUE",
            &[Type::TEXT, Type::TEXT, Type::TEXT],
            &[&org_name, &repo_name, &tag_name]
        )?;
        Ok(row.map(|r| r.into()))
    }
}
