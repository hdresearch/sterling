use axum::{
    Extension, Json,
    extract::{DefaultBodyLimit, Multipart, Path, Query},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use utoipa::{IntoParams, OpenApi};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use dto_lib::ErrorResponse;
use vers_config::VersConfig;

use crate::{
    action::{
        self, BaseImageStatusResponse, CreateBaseImage, CreateBaseImageRequest,
        CreateBaseImageResponse, DeleteBaseImage, DeleteBaseImageResponse, GetBaseImageStatus,
        ListBaseImages, ListBaseImagesResponse, UploadBaseImage, UploadBaseImageRequest,
        UploadBaseImageResponse,
    },
    inbound::{InboundState, OperationId, extractors::AuthApiKey},
};

#[derive(Debug, Deserialize, IntoParams)]
pub struct ListImagesQuery {
    /// Maximum number of images to return (default: 50, max: 100)
    pub limit: Option<i64>,
    /// Number of images to skip (default: 0)
    pub offset: Option<i64>,
}

macro_rules! action_http {
    ($ac:expr) => {
        match $ac {
            Ok(ok) => Ok(ok),
            Err(err) => match err.try_extract_err() {
                Some(err) => Err(err),
                None => return ErrorResponse::internal_server_error(None).into_response(),
            },
        }
    };
}

#[utoipa::path(
    get,
    path = "/",
    params(ListImagesQuery),
    responses(
        (status = 200, description = "List of base images", body = ListBaseImagesResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "images"
)]
pub async fn list_images(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Query(query): Query<ListImagesQuery>,
) -> impl IntoResponse {
    match ListBaseImages::new(key, query.limit, query.offset)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/create",
    request_body = CreateBaseImageRequest,
    responses(
        (status = 201, description = "Image creation job started", body = CreateBaseImageResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Image name already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "images"
)]
pub async fn create_image(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<CreateBaseImageRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            CreateBaseImage::new(key, req).with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{image_name}/status",
    params(("image_name" = String, Path, description = "Image name")),
    responses(
        (status = 200, description = "Image status", body = BaseImageStatusResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Image not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "images"
)]
pub async fn get_image_status(
    AuthApiKey(key): AuthApiKey,
    Path(image_name): Path<String>,
) -> impl IntoResponse {
    match action_http!(action::call(GetBaseImageStatus::new(key, image_name)).await) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Query parameters for image upload
#[derive(Debug, Deserialize, IntoParams)]
pub struct UploadImageQuery {
    /// The name for the new base image
    pub image_name: String,
    /// Size of the image in MiB (default: 512, min: 512, max: 32768)
    pub size_mib: Option<i32>,
}

/// Default image size in MiB
const DEFAULT_IMAGE_SIZE_MIB: i32 = 512;

#[utoipa::path(
    post,
    path = "/upload",
    params(UploadImageQuery),
    responses(
        (status = 201, description = "Image upload started", body = UploadBaseImageResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Image name already exists", body = ErrorResponse),
        (status = 413, description = "Upload too large", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "images"
)]
pub async fn upload_image(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Query(query): Query<UploadImageQuery>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let image_name = query.image_name.clone();
    let size_mib = query.size_mib.unwrap_or(DEFAULT_IMAGE_SIZE_MIB);
    let request_id = Some(operation_id.as_str().to_string());

    // Stream the upload to a temporary file first
    let temp_dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return ErrorResponse::internal_server_error(Some(format!(
                "Failed to create temp directory: {}",
                e
            )))
            .into_response();
        }
    };

    let tarball_path = temp_dir.path().join("upload.tar");
    let mut total_size: u64 = 0;
    let mut file_received = false;

    // Stream the upload to disk
    while let Some(field_result) = multipart.next_field().await.transpose() {
        let field = match field_result {
            Ok(f) => f,
            Err(e) => {
                return ErrorResponse::bad_request(Some(format!("Failed to read upload: {}", e)))
                    .into_response();
            }
        };

        let field_name = field.name().unwrap_or("").to_string();

        // Accept field named "file" or "tarball"
        if field_name != "file" && field_name != "tarball" {
            continue;
        }

        file_received = true;
        tracing::info!(field_name = %field_name, image_name = %image_name, "Receiving tarball upload");

        // Create the output file
        let mut file = match tokio::fs::File::create(&tarball_path).await {
            Ok(f) => f,
            Err(e) => {
                return ErrorResponse::internal_server_error(Some(format!(
                    "Failed to create temp file: {}",
                    e
                )))
                .into_response();
            }
        };

        // Stream chunks to file, checking size limit
        let mut stream = field;
        while let Some(chunk_result) = stream.chunk().await.transpose() {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    return ErrorResponse::bad_request(Some(format!(
                        "Failed to read upload: {}",
                        e
                    )))
                    .into_response();
                }
            };

            total_size += chunk.len() as u64;

            if let Err(e) = file.write_all(&chunk).await {
                return ErrorResponse::internal_server_error(Some(format!(
                    "Failed to write upload: {}",
                    e
                )))
                .into_response();
            }
        }

        if let Err(e) = file.flush().await {
            return ErrorResponse::internal_server_error(Some(format!(
                "Failed to flush upload: {}",
                e
            )))
            .into_response();
        }

        tracing::info!(
            image_name = %image_name,
            total_size = %total_size,
            "Upload received, processing"
        );
        break;
    }

    if !file_received {
        return ErrorResponse::bad_request(Some(
            "No file field found in upload. Use field name 'file' or 'tarball'.".to_string(),
        ))
        .into_response();
    }

    // Now call the upload action
    let request = UploadBaseImageRequest {
        image_name: image_name.clone(),
        size_mib,
        tarball_path: tarball_path.clone(),
        tarball_size: total_size,
    };

    match action_http!(
        action::call(UploadBaseImage::new(key, request).with_request_id(request_id)).await
    ) {
        Ok(response) => {
            // Clean up temp file (action has already streamed it to Chelsea)
            drop(temp_dir);
            (StatusCode::CREATED, Json(response)).into_response()
        }
        Err(e) => {
            // Clean up temp file
            drop(temp_dir);
            e.into_response()
        }
    }
}

#[utoipa::path(
    delete,
    path = "/images/{base_image_id}",
    params(("base_image_id" = String, Path, description = "Base image ID (UUID)")),
    responses(
        (status = 200, description = "Image deleted successfully", body = DeleteBaseImageResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - not owner of image", body = ErrorResponse),
        (status = 404, description = "Image not found", body = ErrorResponse),
        (status = 409, description = "Image is in use by VMs", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "images"
)]
pub async fn delete_image(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(base_image_id): Path<String>,
) -> impl IntoResponse {
    // Parse the base_image_id as UUID
    let base_image_id = match Uuid::parse_str(&base_image_id) {
        Ok(id) => id,
        Err(_) => {
            return ErrorResponse::bad_request(Some("Invalid base_image_id format".to_string()))
                .into_response();
        }
    };

    match action_http!(
        action::call(
            DeleteBaseImage::new(key, base_image_id)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_images,
        create_image,
        get_image_status,
        upload_image,
        delete_image
    ),
    components(schemas(
        ListBaseImagesResponse,
        CreateBaseImageRequest,
        CreateBaseImageResponse,
        BaseImageStatusResponse,
        UploadBaseImageResponse,
        DeleteBaseImageResponse,
        ErrorResponse
    ))
)]
pub struct ImagesApiDoc;

pub fn images_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(ImagesApiDoc::openapi())
        .routes(routes!(list_images))
        .routes(routes!(create_image))
        .routes(routes!(get_image_status))
        .routes(routes!(upload_image))
        .routes(routes!(delete_image))
        .layer(DefaultBodyLimit::max(
            VersConfig::chelsea().image_upload_max_body_bytes,
        ))
}
