use thiserror::Error;

use crate::{
    action::{AuthzError, check_resource_ownership},
    db::{ApiKeyEntity, CommitRepositoriesRepository, CommitTagsRepository, DBError},
};

#[derive(Debug, Clone)]
pub struct DeleteRepoTag {
    pub repo_name: String,
    pub tag_name: String,
    pub api_key: ApiKeyEntity,
}

impl DeleteRepoTag {
    pub fn new(repo_name: String, tag_name: String, api_key: ApiKeyEntity) -> Self {
        Self {
            repo_name,
            tag_name,
            api_key,
        }
    }
}

#[derive(Debug, Error)]
pub enum DeleteRepoTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    RepositoryNotFound,
    #[error("tag not found")]
    TagNotFound,
    #[error("forbidden")]
    Forbidden,
}

impl DeleteRepoTag {
    pub async fn call(self, db: &crate::action::DB) -> Result<(), DeleteRepoTagError> {
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.repo_name)
            .await?
            .ok_or(DeleteRepoTagError::RepositoryNotFound)?;

        let tag = db
            .commit_tags()
            .get_by_repo_and_name(repo.id, &self.tag_name)
            .await?
            .ok_or(DeleteRepoTagError::TagNotFound)?;

        // Check org-level ownership
        check_resource_ownership(&db, &self.api_key, tag.owner_id)
            .await
            .map_err(|e| match e {
                AuthzError::Forbidden => DeleteRepoTagError::Forbidden,
                AuthzError::Db(db) => DeleteRepoTagError::Db(db),
                _ => DeleteRepoTagError::Forbidden,
            })?;

        db.commit_tags().delete(tag.id).await?;

        tracing::info!(
            tag_id = %tag.id,
            repo = %self.repo_name,
            tag = %self.tag_name,
            "Deleted repo tag"
        );

        Ok(())
    }
}

impl_error_response!(DeleteRepoTagError,
    DeleteRepoTagError::Db(_) => INTERNAL_SERVER_ERROR,
    DeleteRepoTagError::RepositoryNotFound => NOT_FOUND,
    DeleteRepoTagError::TagNotFound => NOT_FOUND,
    DeleteRepoTagError::Forbidden => FORBIDDEN,
);
