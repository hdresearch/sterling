use dto_lib::orchestrator::commit_tag::CreateTagResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    action::{AuthzError, check_commit_access},
    db::{ApiKeyEntity, CommitTagsRepository, DB, DBError},
};

#[derive(Debug, Clone)]
pub struct CreateTag {
    pub tag_name: String,
    pub commit_id: Uuid,
    pub description: Option<String>,
    pub api_key: ApiKeyEntity,
}

impl CreateTag {
    pub fn new(
        tag_name: String,
        commit_id: Uuid,
        description: Option<String>,
        api_key: ApiKeyEntity,
    ) -> Self {
        Self {
            tag_name,
            commit_id,
            description,
            api_key,
        }
    }

    /// Validate tag name: alphanumeric, hyphens, underscores, dots, 1-64 chars
    fn validate_tag_name(name: &str) -> Result<(), CreateTagError> {
        if name.is_empty() || name.len() > 64 {
            return Err(CreateTagError::InvalidTagName(
                "Tag name must be 1-64 characters".to_string(),
            ));
        }

        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(CreateTagError::InvalidTagName(
                "Tag name can only contain alphanumeric characters, hyphens, underscores, and dots"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CreateTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("commit not found")]
    CommitNotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("tag already exists")]
    TagAlreadyExists,
    #[error("invalid tag name: {0}")]
    InvalidTagName(String),
}

impl CreateTag {
    pub async fn call(self, db: &DB) -> Result<CreateTagResponse, CreateTagError> {
        // 1. Validate tag name
        Self::validate_tag_name(&self.tag_name)?;

        // 2. Check that user has org-level access to the commit
        let _commit = check_commit_access(db, &self.api_key, self.commit_id)
            .await
            .map_err(|e| match e {
                AuthzError::CommitNotFound => CreateTagError::CommitNotFound,
                AuthzError::Forbidden => CreateTagError::Forbidden,
                AuthzError::Db(db) => CreateTagError::Db(db),
                AuthzError::VmNotFound | AuthzError::TagNotFound => CreateTagError::Forbidden,
            })?;

        // 3. Insert the tag
        let tag = match db
            .commit_tags()
            .insert(
                self.tag_name.clone(),
                self.commit_id,
                self.api_key.id(),
                self.api_key.org_id(),
                self.description,
            )
            .await
        {
            Ok(tag) => tag,
            Err(e) => {
                // Check if error is due to unique constraint violation
                if let Some(db_err) = e.as_db_error() {
                    if db_err.constraint().is_some_and(|c| {
                        c == "unique_tag_per_org" || c == "unique_tag_per_org_legacy"
                    }) {
                        return Err(CreateTagError::TagAlreadyExists);
                    }
                }
                return Err(CreateTagError::Db(e));
            }
        };

        tracing::info!(
            tag_id = %tag.id,
            tag_name = %tag.tag_name,
            commit_id = %tag.commit_id,
            org_id = %tag.org_id,
            "Created commit tag"
        );

        Ok(CreateTagResponse {
            tag_id: tag.id,
            tag_name: tag.tag_name,
            commit_id: tag.commit_id,
        })
    }
}

impl_error_response!(CreateTagError,
    CreateTagError::Db(_) => INTERNAL_SERVER_ERROR,
    CreateTagError::CommitNotFound => NOT_FOUND,
    CreateTagError::Forbidden => FORBIDDEN,
    CreateTagError::TagAlreadyExists => CONFLICT,
    CreateTagError::InvalidTagName(_) => BAD_REQUEST,
);
