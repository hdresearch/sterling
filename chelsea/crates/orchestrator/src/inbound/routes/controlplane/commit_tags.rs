use axum::{Extension, Json, extract::Path, http::StatusCode, response::IntoResponse};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use dto_lib::orchestrator::commit_tag::{
    CreateTagRequest, CreateTagResponse, ListTagsResponse, TagInfo, UpdateTagRequest,
};

use crate::{
    action::{CreateTag, DeleteTag, GetTag, ListTags, UpdateTag},
    inbound::{InboundState, extractors::AuthApiKey},
};

#[utoipa::path(
    post,
    path = "",
    request_body = CreateTagRequest,
    responses(
        (status = 201, description = "Tag created successfully", body = CreateTagResponse),
        (status = 400, description = "Invalid request", body = dto_lib::ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Commit not found", body = dto_lib::ErrorResponse),
        (status = 409, description = "Tag already exists", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commit_tags"
)]
pub async fn create_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<CreateTagRequest>,
) -> impl IntoResponse {
    match CreateTag::new(req.tag_name, req.commit_id, req.description, key)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "",
    responses(
        (status = 200, description = "List of tags", body = ListTagsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commit_tags"
)]
pub async fn list_tags(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
) -> impl IntoResponse {
    match ListTags::new(key).call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{tag_name}",
    params(("tag_name" = String, Path, description = "Tag name")),
    responses(
        (status = 200, description = "Tag details", body = TagInfo),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Tag not found", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commit_tags"
)]
pub async fn get_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(tag_name): Path<String>,
) -> impl IntoResponse {
    match GetTag::new(tag_name, key).call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/{tag_name}",
    request_body = UpdateTagRequest,
    params(("tag_name" = String, Path, description = "Tag name")),
    responses(
        (status = 204, description = "Tag updated"),
        (status = 400, description = "Invalid request", body = dto_lib::ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Tag or commit not found", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commit_tags"
)]
pub async fn update_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(tag_name): Path<String>,
    Json(req): Json<UpdateTagRequest>,
) -> impl IntoResponse {
    match UpdateTag::new(tag_name, req.commit_id, req.description, key)
        .call(&state.db)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/{tag_name}",
    params(("tag_name" = String, Path, description = "Tag name")),
    responses(
        (status = 204, description = "Tag deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Tag not found", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commit_tags"
)]
pub async fn delete_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(tag_name): Path<String>,
) -> impl IntoResponse {
    match DeleteTag::new(tag_name, key).call(&state.db).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(create_tag, list_tags, get_tag, update_tag, delete_tag),
    components(schemas(
        CreateTagRequest,
        CreateTagResponse,
        TagInfo,
        ListTagsResponse,
        UpdateTagRequest,
    ))
)]
pub struct CommitTagsApiDoc;

pub fn commit_tags_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(CommitTagsApiDoc::openapi())
        .routes(routes!(create_tag))
        .routes(routes!(list_tags))
        .routes(routes!(get_tag))
        .routes(routes!(update_tag))
        .routes(routes!(delete_tag))
}
