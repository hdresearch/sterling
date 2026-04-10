use dto_lib::orchestrator::commit_tag::{ListTagsResponse, TagInfo};
use thiserror::Error;

use crate::db::{ApiKeyEntity, CommitTagsRepository, DB, DBError};

#[derive(Debug, Clone)]
pub struct ListTags {
    pub api_key: ApiKeyEntity,
}

impl ListTags {
    pub fn new(api_key: ApiKeyEntity) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Error)]
pub enum ListTagsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
}

impl ListTags {
    pub async fn call(self, db: &DB) -> Result<ListTagsResponse, ListTagsError> {
        // 1. List all tags for the user's organization
        let tags = db.commit_tags().list_by_org(self.api_key.org_id()).await?;

        // 2. Convert to TagInfo DTOs
        let tag_infos: Vec<TagInfo> = tags
            .into_iter()
            .map(|tag| TagInfo {
                tag_id: tag.id,
                tag_name: tag.tag_name,
                commit_id: tag.commit_id,
                description: tag.description,
                created_at: tag.created_at,
                updated_at: tag.updated_at,
            })
            .collect();

        Ok(ListTagsResponse { tags: tag_infos })
    }
}

impl_error_response!(ListTagsError,
    ListTagsError::Db(_) => INTERNAL_SERVER_ERROR,
);
