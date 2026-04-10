use dto_lib::orchestrator::commit_repository::RepoTagInfo;
use thiserror::Error;

use crate::db::{CommitTagsRepository, DBError};

#[derive(Debug, Clone)]
pub struct GetPublicRepoTag {
    pub org_name: String,
    pub repo_name: String,
    pub tag_name: String,
}

impl GetPublicRepoTag {
    pub fn new(org_name: String, repo_name: String, tag_name: String) -> Self {
        Self {
            org_name,
            repo_name,
            tag_name,
        }
    }
}

#[derive(Debug, Error)]
pub enum GetPublicRepoTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("tag not found")]
    NotFound,
}

impl GetPublicRepoTag {
    pub async fn call(self, db: &crate::action::DB) -> Result<RepoTagInfo, GetPublicRepoTagError> {
        let tag = db
            .commit_tags()
            .resolve_public_ref(&self.org_name, &self.repo_name, &self.tag_name)
            .await?
            .ok_or(GetPublicRepoTagError::NotFound)?;

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

impl_error_response!(GetPublicRepoTagError,
    GetPublicRepoTagError::Db(_) => INTERNAL_SERVER_ERROR,
    GetPublicRepoTagError::NotFound => NOT_FOUND,
);
