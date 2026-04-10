use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::db::{ApiKeyEntity, BaseImageEntity, BaseImagesRepository, DB, DBError};

pub const DEFAULT_PAGE_SIZE: i64 = 50;
pub const MAX_PAGE_SIZE: i64 = 100;

pub struct ListBaseImages {
    key: ApiKeyEntity,
    limit: i64,
    offset: i64,
}

impl ListBaseImages {
    pub fn new(key: ApiKeyEntity, limit: Option<i64>, offset: Option<i64>) -> Self {
        let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE).max(1);
        let offset = offset.unwrap_or(0).max(0);
        Self { key, limit, offset }
    }
}

#[derive(Debug, Error)]
pub enum ListBaseImagesError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("forbidden")]
    Forbidden,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BaseImageInfo {
    pub base_image_id: String,
    pub image_name: String,
    pub owner_id: String,
    pub is_public: bool,
    pub source_type: String,
    pub size_mib: i32,
    pub description: Option<String>,
    pub created_at: String,
}

impl From<BaseImageEntity> for BaseImageInfo {
    fn from(entity: BaseImageEntity) -> Self {
        Self {
            base_image_id: entity.base_image_id.to_string(),
            image_name: entity.image_name,
            owner_id: entity.owner_id.to_string(),
            is_public: entity.is_public,
            source_type: entity.source.source_type().to_string(),
            size_mib: entity.size_mib,
            description: entity.description,
            created_at: entity.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListBaseImagesResponse {
    pub images: Vec<BaseImageInfo>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

impl ListBaseImages {
    pub async fn call(self, db: &DB) -> Result<ListBaseImagesResponse, ListBaseImagesError> {
        let owner_id = self.key.id();

        let base_images = db.base_images();
        let (images_result, total_result) = tokio::join!(
            base_images.list_visible_to_owner(owner_id, self.limit, self.offset),
            base_images.count_visible_to_owner(owner_id),
        );

        let images = images_result?;
        let total = total_result?;

        Ok(ListBaseImagesResponse {
            images: images.into_iter().map(BaseImageInfo::from).collect(),
            total,
            limit: self.limit,
            offset: self.offset,
        })
    }
}

impl_error_response!(ListBaseImagesError,
    ListBaseImagesError::Db(_) => INTERNAL_SERVER_ERROR,
    ListBaseImagesError::Forbidden => FORBIDDEN,
);
