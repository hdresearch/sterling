use thiserror::Error;

use crate::{
    action::{AuthzError, check_resource_ownership},
    db::{ApiKeyEntity, CommitTagsRepository, DB, DBError},
};

#[derive(Debug, Clone)]
pub struct DeleteTag {
    pub tag_name: String,
    pub api_key: ApiKeyEntity,
}

impl DeleteTag {
    pub fn new(tag_name: String, api_key: ApiKeyEntity) -> Self {
        Self { tag_name, api_key }
    }
}

#[derive(Debug, Error)]
pub enum DeleteTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("tag not found")]
    TagNotFound,
    #[error("forbidden")]
    Forbidden,
}

impl DeleteTag {
    pub async fn call(self, db: &DB) -> Result<(), DeleteTagError> {
        // 1. Look up tag by org_id and name
        let tag = db
            .commit_tags()
            .get_by_name(self.api_key.org_id(), &self.tag_name)
            .await?
            .ok_or(DeleteTagError::TagNotFound)?;

        // 2. Check org-level access
        check_resource_ownership(db, &self.api_key, tag.owner_id)
            .await
            .map_err(|e| match e {
                AuthzError::Forbidden => DeleteTagError::Forbidden,
                AuthzError::Db(db) => DeleteTagError::Db(db),
                AuthzError::VmNotFound | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    DeleteTagError::Forbidden
                }
            })?;

        // 3. Delete the tag
        db.commit_tags().delete(tag.id).await?;

        tracing::info!(
            tag_id = %tag.id,
            tag_name = %tag.tag_name,
            commit_id = %tag.commit_id,
            "Deleted commit tag"
        );

        Ok(())
    }
}

impl_error_response!(DeleteTagError,
    DeleteTagError::Db(_) => INTERNAL_SERVER_ERROR,
    DeleteTagError::TagNotFound => NOT_FOUND,
    DeleteTagError::Forbidden => FORBIDDEN,
);
