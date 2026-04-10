use dto_lib::orchestrator::commit_repository::PublicRepositoryInfo;
use thiserror::Error;

use crate::db::{CommitRepositoriesRepository, DBError};

#[derive(Debug, Clone)]
pub struct GetPublicRepository {
    pub org_name: String,
    pub repo_name: String,
}

impl GetPublicRepository {
    pub fn new(org_name: String, repo_name: String) -> Self {
        Self {
            org_name,
            repo_name,
        }
    }
}

#[derive(Debug, Error)]
pub enum GetPublicRepositoryError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    NotFound,
}

impl GetPublicRepository {
    pub async fn call(
        self,
        db: &crate::action::DB,
    ) -> Result<PublicRepositoryInfo, GetPublicRepositoryError> {
        let repo = db
            .commit_repositories()
            .get_public_by_org_and_name(&self.org_name, &self.repo_name)
            .await?
            .ok_or(GetPublicRepositoryError::NotFound)?;

        let full_name = format!("{}/{}", self.org_name, repo.name);

        Ok(PublicRepositoryInfo {
            repo_id: repo.id,
            org_name: self.org_name,
            name: repo.name,
            full_name,
            description: repo.description,
            created_at: repo.created_at,
        })
    }
}

impl_error_response!(GetPublicRepositoryError,
    GetPublicRepositoryError::Db(_) => INTERNAL_SERVER_ERROR,
    GetPublicRepositoryError::NotFound => NOT_FOUND,
);
