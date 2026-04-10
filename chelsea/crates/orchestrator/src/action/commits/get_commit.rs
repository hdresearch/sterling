use thiserror::Error;
use uuid::Uuid;

use crate::action::authz::check_commit_read_access;
use crate::db::{ApiKeyEntity, DB, DBError, VmCommitEntity};
use crate::outbound::node_proto::HttpError;

/// For internal use. this shouldn't return outside of orchestrator.
#[derive(Debug, Clone)]
pub struct GetCommit {
    pub commit_id: Uuid,
    pub api_key: ApiKeyEntity,
}
impl GetCommit {
    pub fn by_id(commit_id: Uuid, api_key: ApiKeyEntity) -> Self {
        Self { commit_id, api_key }
    }
}

#[derive(Debug, Error)]
pub enum GetCommitError {
    #[error("db error: {0}")]
    Db(#[from] DBError),

    #[error("db error: {0}")]
    Http(#[from] HttpError),
    #[error("not found")]
    NotFound,

    #[error("forbidden")]
    Forbidden,

    #[error("internal server error")]
    InternalServerError,
}

impl GetCommit {
    pub async fn call(self, db: &DB) -> Result<VmCommitEntity, GetCommitError> {
        // Fetch and check read access (allows public commits)
        let commit = check_commit_read_access(db, &self.api_key, self.commit_id)
            .await
            .map_err(|e| match e {
                crate::action::AuthzError::CommitNotFound => GetCommitError::NotFound,
                crate::action::AuthzError::Forbidden => GetCommitError::Forbidden,
                crate::action::AuthzError::Db(db) => GetCommitError::Db(db),
                _ => GetCommitError::Forbidden,
            })?;

        Ok(commit)
    }
}

impl_error_response!(GetCommitError,
    GetCommitError::Db(_) => INTERNAL_SERVER_ERROR,
    GetCommitError::Http(_) => INTERNAL_SERVER_ERROR,
    GetCommitError::NotFound => NOT_FOUND,
    GetCommitError::Forbidden => FORBIDDEN,
    GetCommitError::InternalServerError => INTERNAL_SERVER_ERROR,
);
