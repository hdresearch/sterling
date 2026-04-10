use thiserror::Error;
use uuid::Uuid;

use crate::action::authz::{AuthzError, check_commit_read_access_entity};
use crate::db::{ApiKeyEntity, DB, DBError, VMCommitsRepository, VmCommitEntity};

#[derive(Debug, Clone)]
pub struct ListParentCommits {
    pub commit_id: Uuid,
    pub api_key: ApiKeyEntity,
}

impl ListParentCommits {
    pub fn new(commit_id: Uuid, api_key: ApiKeyEntity) -> Self {
        Self { commit_id, api_key }
    }
}

#[derive(Debug, Error)]
pub enum ListParentCommitsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),

    #[error("commit not found")]
    NotFound,

    #[error("forbidden")]
    Forbidden,
}

impl ListParentCommits {
    pub async fn call(self, db: &DB) -> Result<Vec<VmCommitEntity>, ListParentCommitsError> {
        // Use a single recursive SQL query to fetch all parent commits efficiently
        let commits = db
            .commits()
            .get_parent_commits_recursive(self.commit_id)
            .await?;

        // Check if any commits were found
        if commits.is_empty() {
            return Err(ListParentCommitsError::NotFound);
        }

        // Verify authorization - check that each commit is either public or belongs to the same org
        for commit in &commits {
            check_commit_read_access_entity(db, &self.api_key, commit)
                .await
                .map_err(|e| match e {
                    AuthzError::Db(db_err) => ListParentCommitsError::Db(db_err),
                    _ => ListParentCommitsError::Forbidden,
                })?;
        }

        Ok(commits)
    }
}

impl_error_response!(ListParentCommitsError,
    ListParentCommitsError::Db(_) => INTERNAL_SERVER_ERROR,
    ListParentCommitsError::NotFound => NOT_FOUND,
    ListParentCommitsError::Forbidden => FORBIDDEN,
);
