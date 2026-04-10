use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub use chelsea_lib::base_image::{CreateBaseImageRequest, ImageCreationStatus, ImageSource};

#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct CreateBaseImageResponse {
    pub image_name: String,
    pub status: ImageCreationStatus,
}

#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct ImageStatusResponse {
    pub image_name: String,
    pub status: ImageCreationStatus,
    pub size_mib: Option<u32>,
}

#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct BaseImageInfo {
    pub image_name: String,
    pub size_mib: u32,
    pub snapshot_name: String,
}

#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct ListImagesResponse {
    pub images: Vec<BaseImageInfo>,
}

/// Query parameters for the image upload endpoint.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct UploadImageQuery {
    pub image_name: String,
    pub size_mib: Option<u32>,
}
