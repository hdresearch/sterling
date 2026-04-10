use dto_lib::orchestrator::commit_repository::RepoTagInfo;
use thiserror::Error;

use crate::db::{ApiKeyEntity, CommitRepositoriesRepository, CommitTagsRepository, DBError};

#[derive(Debug, Clone)]
pub struct GetRepoTag {
    pub repo_name: String,
    pub tag_name: String,
    pub api_key: ApiKeyEntity,
}

impl GetRepoTag {
    pub fn new(repo_name: String, tag_name: String, api_key: ApiKeyEntity) -> Self {
        Self {
            repo_name,
            tag_name,
            api_key,
        }
    }
}

#[derive(Debug, Error)]
pub enum GetRepoTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    RepositoryNotFound,
    #[error("tag not found")]
    TagNotFound,
}

impl GetRepoTag {
    pub async fn call(self, db: &crate::action::DB) -> Result<RepoTagInfo, GetRepoTagError> {
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.repo_name)
            .await?
            .ok_or(GetRepoTagError::RepositoryNotFound)?;

        let tag = db
            .commit_tags()
            .get_by_repo_and_name(repo.id, &self.tag_name)
            .await?
            .ok_or(GetRepoTagError::TagNotFound)?;

        Ok(RepoTagInfo {
            tag_id: tag.id,
            tag_name: tag.tag_name.clone(),
            reference: format!("{}:{}", self.repo_name, tag.tag_name),
            commit_id: tag.commit_id,
            description: tag.description,
            created_at: tag.created_at,
            updated_at: tag.updated_at,
        })
    }
}

impl_error_response!(GetRepoTagError,
    GetRepoTagError::Db(_) => INTERNAL_SERVER_ERROR,
    GetRepoTagError::RepositoryNotFound => NOT_FOUND,
    GetRepoTagError::TagNotFound => NOT_FOUND,
);
