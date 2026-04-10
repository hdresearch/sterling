use thiserror::Error;
use uuid::Uuid;

use crate::{
    action::{AuthzError, check_commit_access},
    db::{ApiKeyEntity, CommitRepositoriesRepository, CommitTagsRepository, DBError},
};

#[derive(Debug, Clone)]
pub struct UpdateRepoTag {
    pub repo_name: String,
    pub tag_name: String,
    pub new_commit_id: Option<Uuid>,
    pub new_description: Option<Option<String>>,
    pub api_key: ApiKeyEntity,
}

impl UpdateRepoTag {
    pub fn new(
        repo_name: String,
        tag_name: String,
        new_commit_id: Option<Uuid>,
        new_description: Option<Option<String>>,
        api_key: ApiKeyEntity,
    ) -> Self {
        Self {
            repo_name,
            tag_name,
            new_commit_id,
            new_description,
            api_key,
        }
    }
}

#[derive(Debug, Error)]
pub enum UpdateRepoTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    RepositoryNotFound,
    #[error("tag not found")]
    TagNotFound,
    #[error("commit not found")]
    CommitNotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("no updates provided")]
    NoUpdatesProvided,
}

impl UpdateRepoTag {
    pub async fn call(self, db: &crate::action::DB) -> Result<(), UpdateRepoTagError> {
        if self.new_commit_id.is_none() && self.new_description.is_none() {
            return Err(UpdateRepoTagError::NoUpdatesProvided);
        }

        // 1. Look up the repository
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.repo_name)
            .await?
            .ok_or(UpdateRepoTagError::RepositoryNotFound)?;

        // 2. Look up the tag within the repository
        let tag = db
            .commit_tags()
            .get_by_repo_and_name(repo.id, &self.tag_name)
            .await?
            .ok_or(UpdateRepoTagError::TagNotFound)?;

        // 3. If updating commit_id, verify access to the new commit
        if let Some(new_commit_id) = self.new_commit_id {
            check_commit_access(&db, &self.api_key, new_commit_id)
                .await
                .map_err(|e| match e {
                    AuthzError::CommitNotFound => UpdateRepoTagError::CommitNotFound,
                    AuthzError::Forbidden => UpdateRepoTagError::Forbidden,
                    AuthzError::Db(db) => UpdateRepoTagError::Db(db),
                    _ => UpdateRepoTagError::Forbidden,
                })?;
        }

        // 4. Perform the update
        db.commit_tags()
            .update(tag.id, self.new_commit_id, self.new_description)
            .await?;

        tracing::info!(
            tag_id = %tag.id,
            repo = %self.repo_name,
            tag = %self.tag_name,
            "Updated repo tag"
        );

        Ok(())
    }
}

impl_error_response!(UpdateRepoTagError,
    UpdateRepoTagError::Db(_) => INTERNAL_SERVER_ERROR,
    UpdateRepoTagError::RepositoryNotFound => NOT_FOUND,
    UpdateRepoTagError::TagNotFound => NOT_FOUND,
    UpdateRepoTagError::CommitNotFound => NOT_FOUND,
    UpdateRepoTagError::Forbidden => FORBIDDEN,
    UpdateRepoTagError::NoUpdatesProvided => BAD_REQUEST,
);
