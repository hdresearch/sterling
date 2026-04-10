use dto_lib::orchestrator::commit_repository::RepositoryInfo;
use thiserror::Error;

use crate::db::{ApiKeyEntity, CommitRepositoriesRepository, DBError};

#[derive(Debug, Clone)]
pub struct GetRepository {
    pub name: String,
    pub api_key: ApiKeyEntity,
}

impl GetRepository {
    pub fn new(name: String, api_key: ApiKeyEntity) -> Self {
        Self { name, api_key }
    }
}

#[derive(Debug, Error)]
pub enum GetRepositoryError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    NotFound,
}

impl GetRepository {
    pub async fn call(self, db: &crate::action::DB) -> Result<RepositoryInfo, GetRepositoryError> {
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.name)
            .await?
            .ok_or(GetRepositoryError::NotFound)?;

        Ok(RepositoryInfo {
            repo_id: repo.id,
            name: repo.name,
            description: repo.description,
            is_public: repo.is_public,
            created_at: repo.created_at,
        })
    }
}

impl_error_response!(GetRepositoryError,
    GetRepositoryError::Db(_) => INTERNAL_SERVER_ERROR,
    GetRepositoryError::NotFound => NOT_FOUND,
);
