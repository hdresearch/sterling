use dto_lib::orchestrator::commit_tag::TagInfo;
use thiserror::Error;

use crate::{
    action::{AuthzError, check_resource_ownership},
    db::{ApiKeyEntity, CommitTagsRepository, DB, DBError},
};

#[derive(Debug, Clone)]
pub struct GetTag {
    pub tag_name: String,
    pub api_key: ApiKeyEntity,
}

impl GetTag {
    pub fn new(tag_name: String, api_key: ApiKeyEntity) -> Self {
        Self { tag_name, api_key }
    }
}

#[derive(Debug, Error)]
pub enum GetTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("tag not found")]
    TagNotFound,
    #[error("forbidden")]
    Forbidden,
}

impl GetTag {
    pub async fn call(self, db: &DB) -> Result<TagInfo, GetTagError> {
        // 1. Look up tag by org_id and name
        let tag = db
            .commit_tags()
            .get_by_name(self.api_key.org_id(), &self.tag_name)
            .await?
            .ok_or(GetTagError::TagNotFound)?;

        // 2. Check org-level access
        check_resource_ownership(db, &self.api_key, tag.owner_id)
            .await
            .map_err(|e| match e {
                AuthzError::Forbidden => GetTagError::Forbidden,
                AuthzError::Db(db) => GetTagError::Db(db),
                AuthzError::VmNotFound | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    GetTagError::Forbidden
                }
            })?;

        Ok(TagInfo {
            tag_id: tag.id,
            tag_name: tag.tag_name,
            commit_id: tag.commit_id,
            description: tag.description,
            created_at: tag.created_at,
            updated_at: tag.updated_at,
        })
    }
}

impl_error_response!(GetTagError,
    GetTagError::Db(_) => INTERNAL_SERVER_ERROR,
    GetTagError::TagNotFound => NOT_FOUND,
    GetTagError::Forbidden => FORBIDDEN,
);
