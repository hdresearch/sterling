use thiserror::Error;
use uuid::Uuid;

use crate::action::authz::check_commit_access;
use crate::db::{ApiKeyEntity, DB, DBError, VMCommitsRepository, VmCommitEntity};

#[derive(Debug, Clone)]
pub struct SetCommitPublic {
    pub commit_id: Uuid,
    pub is_public: bool,
    pub name: Option<String>,
    pub description: Option<String>,
    pub api_key: ApiKeyEntity,
}

impl SetCommitPublic {
    pub fn new(commit_id: Uuid, is_public: bool, api_key: ApiKeyEntity) -> Self {
        Self {
            commit_id,
            is_public,
            name: None,
            description: None,
            api_key,
        }
    }

    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }
}

#[derive(Debug, Error)]
pub enum SetCommitPublicError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
}

impl SetCommitPublic {
    pub async fn call(self, db: &DB) -> Result<VmCommitEntity, SetCommitPublicError> {
        // Only the owner (same org) can toggle public visibility
        let commit = check_commit_access(db, &self.api_key, self.commit_id)
            .await
            .map_err(|e| match e {
                crate::action::AuthzError::CommitNotFound => SetCommitPublicError::NotFound,
                crate::action::AuthzError::Forbidden => SetCommitPublicError::Forbidden,
                crate::action::AuthzError::Db(db) => SetCommitPublicError::Db(db),
                _ => SetCommitPublicError::Forbidden,
            })?;

        // Additionally, only the key that owns the commit can set public
        if commit.owner_id != self.api_key.id() {
            return Err(SetCommitPublicError::Forbidden);
        }

        db.commits()
            .update_metadata(self.commit_id, self.is_public, self.name, self.description)
            .await?;

        // Return updated entity
        let updated = db
            .commits()
            .get_by_id(self.commit_id)
            .await?
            .ok_or(SetCommitPublicError::NotFound)?;

        Ok(updated)
    }
}

impl_error_response!(SetCommitPublicError,
    SetCommitPublicError::Db(_) => INTERNAL_SERVER_ERROR,
    SetCommitPublicError::NotFound => NOT_FOUND,
    SetCommitPublicError::Forbidden => FORBIDDEN,
);
