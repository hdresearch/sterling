use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::StatusCode,
};
use chelsea_lib::base_image::{
    BaseImageBuilder, BaseImageError, CreateBaseImageRequest, ImageCreationStatus, ImageSource,
    base_image_exists, delete_base_image, list_base_images,
};
use dto_lib::chelsea_server2::images::{
    BaseImageInfo, CreateBaseImageResponse, ImageStatusResponse, ListImagesResponse,
    UploadImageQuery,
};
use tokio::io::AsyncWriteExt;
use tracing::{error, info};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};
use vers_config::VersConfig;

use crate::ChelseaServerCore;

#[utoipa::path(
    post,
    path = "/api/images/create",
    request_body = CreateBaseImageRequest,
    responses(
        (status = 200, description = "Base image creation started", body = CreateBaseImageResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_base_image_handler(
    State(_core): State<Arc<dyn ChelseaServerCore>>,
    Json(request): Json<CreateBaseImageRequest>,
) -> Result<Json<CreateBaseImageResponse>, (StatusCode, String)> {
    let image_name = request.image_name.clone();
    info!(%image_name, "Received base image creation request");

    // Note: We don't pre-check for image existence here to avoid a race condition.
    // The builder will handle the case where the image already exists by setting
    // status to Failed, which clients can observe by polling the status endpoint.

    // Start the image creation in a background task
    let builder = BaseImageBuilder::new();
    let request_clone = request.clone();

    tokio::spawn(async move {
        if let Err(e) = builder.create(&request_clone).await {
            error!(image_name = %request_clone.image_name, %e, "Base image creation failed");
        }
    });

    Ok(Json(CreateBaseImageResponse {
        image_name,
        status: ImageCreationStatus::Pending,
    }))
}

/// Get the status of a base image
#[utoipa::path(
    get,
    path = "/api/images/{image_name}/status",
    params(
        ("image_name" = String, Path, description = "The name of the base image")
    ),
    responses(
        (status = 200, description = "Image status retrieved", body = ImageStatusResponse),
        (status = 404, description = "Image not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_image_status_handler(
    State(_core): State<Arc<dyn ChelseaServerCore>>,
    Path(image_name): Path<String>,
) -> Result<Json<ImageStatusResponse>, (StatusCode, String)> {
    info!(%image_name, "Checking base image status");

    // Check if the base image exists (has the chelsea_base_image snapshot)
    match base_image_exists(&image_name).await {
        Ok(true) => {
            // Image exists and is complete
            let client = ceph::default_rbd_client().map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to get RBD client: {}", e),
                )
            })?;

            let size_mib = client
                .image_info(&image_name)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to get image size: {}", e),
                    )
                })?
                .size_mib();

            Ok(Json(ImageStatusResponse {
                image_name,
                status: ImageCreationStatus::Completed,
                size_mib: Some(size_mib),
            }))
        }
        Ok(false) => {
            // Check if the image exists at all (might be in progress)
            let client = ceph::default_rbd_client().map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to get RBD client: {}", e),
                )
            })?;

            match client.image_exists(&image_name).await {
                Ok(true) => {
                    // Image exists but no base snapshot - might be in progress
                    Ok(Json(ImageStatusResponse {
                        image_name,
                        status: ImageCreationStatus::CreatingRbd,
                        size_mib: None,
                    }))
                }
                Ok(false) => Err((
                    StatusCode::NOT_FOUND,
                    format!("Image '{}' not found", image_name),
                )),
                Err(e) => Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to check image existence: {}", e),
                )),
            }
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to check base image: {}", e),
        )),
    }
}

/// List all available base images
#[utoipa::path(
    get,
    path = "/api/images",
    responses(
        (status = 200, description = "List of available base images", body = ListImagesResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_images_handler(
    State(_core): State<Arc<dyn ChelseaServerCore>>,
) -> Result<Json<ListImagesResponse>, (StatusCode, String)> {
    info!("Listing base images");

    let image_names = list_base_images().await.map_err(|e| {
        error!(%e, "Failed to list base images");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list base images: {}", e),
        )
    })?;

    let client = ceph::default_rbd_client().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get RBD client: {}", e),
        )
    })?;

    let mut images = Vec::new();
    for name in image_names {
        let size = match client.image_info(&name).await {
            Ok(info) => info.size_mib(),
            Err(e) => {
                error!(image_name = %name, %e, "Failed to get image size, skipping");
                continue;
            }
        };

        images.push(BaseImageInfo {
            image_name: name,
            size_mib: size,
            snapshot_name: VersConfig::chelsea().ceph_base_image_snap_name.clone(),
        });
    }

    Ok(Json(ListImagesResponse { images }))
}

#[utoipa::path(
    post,
    path = "/api/images/upload",
    params(
        ("image_name" = String, Query, description = "The name for the new base image"),
        ("size_mib" = Option<u32>, Query, description = "Size of the image in MiB (default: 512)")
    ),
    responses(
        (status = 200, description = "Upload started, image creation in progress", body = CreateBaseImageResponse),
        (status = 400, description = "Bad request - missing file or invalid parameters"),
        (status = 409, description = "Image already exists"),
        (status = 413, description = "File too large (max 10GB)"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn upload_image_handler(
    State(_core): State<Arc<dyn ChelseaServerCore>>,
    Query(query): Query<UploadImageQuery>,
    mut multipart: Multipart,
) -> Result<Json<CreateBaseImageResponse>, (StatusCode, String)> {
    let image_name = query.image_name.clone();
    info!(%image_name, "Received image upload request");

    // Check if image already exists
    match base_image_exists(&image_name).await {
        Ok(true) => {
            return Err((
                StatusCode::CONFLICT,
                format!("Image '{}' already exists", image_name),
            ));
        }
        Ok(false) => {}
        Err(e) => {
            error!(%e, "Failed to check if image exists");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to check if image exists: {}", e),
            ));
        }
    }

    // Create a temporary file to store the upload
    let temp_dir = tempfile::tempdir().map_err(|e| {
        error!(%e, "Failed to create temp directory for upload");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create temp directory: {}", e),
        )
    })?;

    let tarball_path = temp_dir.path().join("upload.tar");
    let mut total_size: u64 = 0;
    let mut file_received = false;

    // Stream the upload to disk
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        error!(%e, "Failed to read multipart field");
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to read upload: {}", e),
        )
    })? {
        let field_name = field.name().unwrap_or("").to_string();

        // Accept field named "file" or "tarball"
        if field_name != "file" && field_name != "tarball" {
            continue;
        }

        file_received = true;
        info!(field_name = %field_name, "Receiving tarball upload");

        // Create the output file
        let mut file = tokio::fs::File::create(&tarball_path).await.map_err(|e| {
            error!(%e, "Failed to create temp file for upload");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create temp file: {}", e),
            )
        })?;

        // Stream chunks to file, checking size limit
        let mut stream = field;
        while let Some(chunk) = stream.chunk().await.map_err(|e| {
            error!(%e, "Failed to read upload chunk");
            (
                StatusCode::BAD_REQUEST,
                format!("Failed to read upload: {}", e),
            )
        })? {
            total_size += chunk.len() as u64;

            file.write_all(&chunk).await.map_err(|e| {
                error!(%e, "Failed to write upload chunk");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to write upload: {}", e),
                )
            })?;
        }

        file.flush().await.map_err(|e| {
            error!(%e, "Failed to flush upload file");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to flush upload: {}", e),
            )
        })?;

        info!(%total_size, "Upload complete, starting image creation");
        break;
    }

    if !file_received {
        return Err((
            StatusCode::BAD_REQUEST,
            "No file field found in upload. Use field name 'file' or 'tarball'.".to_string(),
        ));
    }

    // Start the image creation in a background task
    let builder = BaseImageBuilder::new();
    let tarball_path_str = tarball_path.to_string_lossy().to_string();
    let image_name_clone = image_name.clone();
    let size_mib = query.size_mib;

    // Keep temp_dir alive in the spawned task so the file doesn't get deleted
    tokio::spawn(async move {
        let _temp_dir = temp_dir; // Keep alive until task completes

        let request = CreateBaseImageRequest {
            image_name: image_name_clone.clone(),
            source: ImageSource::Upload {
                tarball_path: tarball_path_str,
            },
            size_mib: size_mib.unwrap_or(256),
        };

        if let Err(e) = builder.create(&request).await {
            error!(image_name = %image_name_clone, %e, "Base image creation from upload failed");
        }

        // temp_dir will be dropped here, cleaning up the uploaded tarball
    });

    Ok(Json(CreateBaseImageResponse {
        image_name,
        status: ImageCreationStatus::Pending,
    }))
}

/// Delete a base image
#[utoipa::path(
    delete,
    path = "/api/images/{image_name}",
    params(
        ("image_name" = String, Path, description = "The name of the base image to delete")
    ),
    responses(
        (status = 204, description = "Image deleted successfully"),
        (status = 404, description = "Image not found"),
        (status = 409, description = "Image is in use by VMs or has child clones"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_image_handler(
    State(_core): State<Arc<dyn ChelseaServerCore>>,
    Path(image_name): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    info!(%image_name, "Received base image deletion request");

    match delete_base_image(&image_name).await {
        Ok(()) => {
            info!(%image_name, "Base image deleted successfully");
            Ok(StatusCode::NO_CONTENT)
        }
        Err(BaseImageError::ImageNotFound(name)) => {
            Err((StatusCode::NOT_FOUND, format!("Image '{}' not found", name)))
        }
        Err(BaseImageError::ImageHasChildClones(name)) => Err((
            StatusCode::CONFLICT,
            format!(
                "Image '{}' has child clones (VMs using it) and cannot be deleted",
                name
            ),
        )),
        Err(BaseImageError::ImageInUse { image_name, vm_ids }) => Err((
            StatusCode::CONFLICT,
            format!("Image '{}' is in use by VMs: {:?}", image_name, vm_ids),
        )),
        Err(e) => {
            error!(%image_name, %e, "Failed to delete base image");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to delete image: {}", e),
            ))
        }
    }
}

#[derive(OpenApi)]
#[openapi(paths(
    create_base_image_handler,
    get_image_status_handler,
    list_images_handler,
    upload_image_handler,
    delete_image_handler,
))]
pub struct ImagesApiDoc;

pub fn create_images_router(
    core: Arc<dyn ChelseaServerCore>,
) -> (Router, utoipa::openapi::OpenApi) {
    let (router, openapi) = OpenApiRouter::with_openapi(ImagesApiDoc::openapi())
        .routes(routes!(create_base_image_handler))
        .routes(routes!(get_image_status_handler))
        .routes(routes!(list_images_handler))
        .routes(routes!(upload_image_handler))
        .routes(routes!(delete_image_handler))
        .layer(DefaultBodyLimit::max(
            VersConfig::chelsea().image_upload_max_body_bytes,
        ))
        .with_state(core)
        .split_for_parts();

    (router, openapi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_doc_has_all_paths() {
        let api = ImagesApiDoc::openapi();
        let paths = api.paths.paths;

        assert!(paths.contains_key("/api/images/create"));
        assert!(paths.contains_key("/api/images/{image_name}/status"));
        assert!(paths.contains_key("/api/images"));
        assert!(paths.contains_key("/api/images/upload"));
        assert!(paths.contains_key("/api/images/{image_name}"));
    }
}
