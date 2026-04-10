use thiserror::Error;
use uuid::Uuid;

use crate::{
    action::{AuthzError, check_commit_access, check_resource_ownership},
    db::{ApiKeyEntity, CommitTagsRepository, DB, DBError},
};

#[derive(Debug, Clone)]
pub struct UpdateTag {
    pub tag_name: String,
    pub new_commit_id: Option<Uuid>,
    /// `None` = don't change description, `Some(None)` = clear it, `Some(Some(s))` = set it
    pub description: Option<Option<String>>,
    pub api_key: ApiKeyEntity,
}

impl UpdateTag {
    pub fn new(
        tag_name: String,
        new_commit_id: Option<Uuid>,
        description: Option<Option<String>>,
        api_key: ApiKeyEntity,
    ) -> Self {
        Self {
            tag_name,
            new_commit_id,
            description,
            api_key,
        }
    }
}

#[derive(Debug, Error)]
pub enum UpdateTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("tag not found")]
    TagNotFound,
    #[error("commit not found")]
    CommitNotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("no updates provided")]
    NoUpdatesProvided,
}

impl UpdateTag {
    pub async fn call(self, db: &DB) -> Result<(), UpdateTagError> {
        // Validate that at least one update is provided
        if self.new_commit_id.is_none() && self.description.is_none() {
            return Err(UpdateTagError::NoUpdatesProvided);
        }

        // 1. Look up tag by org_id and name
        let tag = db
            .commit_tags()
            .get_by_name(self.api_key.org_id(), &self.tag_name)
            .await?
            .ok_or(UpdateTagError::TagNotFound)?;

        // 2. Check org-level access to tag
        check_resource_ownership(db, &self.api_key, tag.owner_id)
            .await
            .map_err(|e| match e {
                AuthzError::Forbidden => UpdateTagError::Forbidden,
                AuthzError::Db(db) => UpdateTagError::Db(db),
                AuthzError::VmNotFound | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    UpdateTagError::Forbidden
                }
            })?;

        // 3. If moving tag to new commit, validate commit exists and user has access
        if let Some(new_commit_id) = self.new_commit_id {
            check_commit_access(db, &self.api_key, new_commit_id)
                .await
                .map_err(|e| match e {
                    AuthzError::CommitNotFound => UpdateTagError::CommitNotFound,
                    AuthzError::Forbidden => UpdateTagError::Forbidden,
                    AuthzError::Db(db) => UpdateTagError::Db(db),
                    AuthzError::VmNotFound | AuthzError::TagNotFound => UpdateTagError::Forbidden,
                })?;
        }

        // 4. Atomically update both fields
        let updated = db
            .commit_tags()
            .update(tag.id, self.new_commit_id, self.description.clone())
            .await?;

        if self.new_commit_id.is_some() {
            tracing::info!(
                tag_id = %tag.id,
                tag_name = %tag.tag_name,
                old_commit_id = %tag.commit_id,
                new_commit_id = %updated.commit_id,
                "Moved tag to new commit"
            );
        }

        if self.description.is_some() {
            tracing::info!(
                tag_id = %tag.id,
                tag_name = %tag.tag_name,
                "Updated tag description"
            );
        }

        Ok(())
    }
}

impl_error_response!(UpdateTagError,
    UpdateTagError::Db(_) => INTERNAL_SERVER_ERROR,
    UpdateTagError::TagNotFound => NOT_FOUND,
    UpdateTagError::CommitNotFound => NOT_FOUND,
    UpdateTagError::Forbidden => FORBIDDEN,
    UpdateTagError::NoUpdatesProvided => BAD_REQUEST,
);
