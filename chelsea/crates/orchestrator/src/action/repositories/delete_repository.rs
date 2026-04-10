use thiserror::Error;

use crate::{
    action::{AuthzError, check_resource_ownership},
    db::{ApiKeyEntity, CommitRepositoriesRepository, DBError},
};

#[derive(Debug, Clone)]
pub struct DeleteRepository {
    pub name: String,
    pub api_key: ApiKeyEntity,
}

impl DeleteRepository {
    pub fn new(name: String, api_key: ApiKeyEntity) -> Self {
        Self { name, api_key }
    }
}

#[derive(Debug, Error)]
pub enum DeleteRepositoryError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
}

impl DeleteRepository {
    pub async fn call(self, db: &crate::action::DB) -> Result<(), DeleteRepositoryError> {
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.name)
            .await?
            .ok_or(DeleteRepositoryError::NotFound)?;

        // Check org-level ownership
        check_resource_ownership(&db, &self.api_key, repo.owner_id)
            .await
            .map_err(|e| match e {
                AuthzError::Forbidden => DeleteRepositoryError::Forbidden,
                AuthzError::Db(db) => DeleteRepositoryError::Db(db),
                _ => DeleteRepositoryError::Forbidden,
            })?;

        // Delete the repository (tags cascade-delete via FK)
        let deleted = db.commit_repositories().delete(repo.id).await?;

        if !deleted {
            return Err(DeleteRepositoryError::NotFound);
        }

        tracing::info!(
            repo_id = %repo.id,
            name = %repo.name,
            org_id = %repo.org_id,
            "Deleted commit repository"
        );

        Ok(())
    }
}

impl_error_response!(DeleteRepositoryError,
    DeleteRepositoryError::Db(_) => INTERNAL_SERVER_ERROR,
    DeleteRepositoryError::NotFound => NOT_FOUND,
    DeleteRepositoryError::Forbidden => FORBIDDEN,
);
