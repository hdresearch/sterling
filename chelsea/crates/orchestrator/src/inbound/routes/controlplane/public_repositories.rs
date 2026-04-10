use axum::{Extension, Json, extract::Path, http::StatusCode, response::IntoResponse};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use dto_lib::{
    ErrorResponse,
    orchestrator::commit_repository::{
        ListPublicRepositoriesResponse, ListRepoTagsResponse, PublicRepositoryInfo, RepoTagInfo,
    },
};

use crate::{
    action::{GetPublicRepoTag, GetPublicRepository, ListPublicRepoTags, ListPublicRepositories},
    inbound::InboundState,
};

// ── Public Repository Browsing (no auth required) ───────────────────────

#[utoipa::path(
    get,
    path = "",
    responses(
        (status = 200, description = "List of public repositories", body = ListPublicRepositoriesResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    tag = "public_repositories"
)]
pub async fn list_public_repositories(
    Extension(state): Extension<InboundState>,
) -> impl IntoResponse {
    match ListPublicRepositories::new().call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{org_name}/{repo_name}",
    params(
        ("org_name" = String, Path, description = "Organization name"),
        ("repo_name" = String, Path, description = "Repository name"),
    ),
    responses(
        (status = 200, description = "Public repository details", body = PublicRepositoryInfo),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    tag = "public_repositories"
)]
pub async fn get_public_repository(
    Extension(state): Extension<InboundState>,
    Path((org_name, repo_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match GetPublicRepository::new(org_name, repo_name)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{org_name}/{repo_name}/tags",
    params(
        ("org_name" = String, Path, description = "Organization name"),
        ("repo_name" = String, Path, description = "Repository name"),
    ),
    responses(
        (status = 200, description = "List of tags in public repository", body = ListRepoTagsResponse),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    tag = "public_repositories"
)]
pub async fn list_public_repo_tags(
    Extension(state): Extension<InboundState>,
    Path((org_name, repo_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match ListPublicRepoTags::new(org_name, repo_name)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{org_name}/{repo_name}/tags/{tag_name}",
    params(
        ("org_name" = String, Path, description = "Organization name"),
        ("repo_name" = String, Path, description = "Repository name"),
        ("tag_name" = String, Path, description = "Tag name"),
    ),
    responses(
        (status = 200, description = "Tag details", body = RepoTagInfo),
        (status = 404, description = "Tag not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    tag = "public_repositories"
)]
pub async fn get_public_repo_tag(
    Extension(state): Extension<InboundState>,
    Path((org_name, repo_name, tag_name)): Path<(String, String, String)>,
) -> impl IntoResponse {
    match GetPublicRepoTag::new(org_name, repo_name, tag_name)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── OpenAPI + Router ────────────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
    paths(
        list_public_repositories,
        get_public_repository,
        list_public_repo_tags,
        get_public_repo_tag,
    ),
    components(schemas(
        ListPublicRepositoriesResponse,
        PublicRepositoryInfo,
        ListRepoTagsResponse,
        RepoTagInfo,
    ))
)]
pub struct PublicRepositoriesApiDoc;

pub fn public_repositories_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(PublicRepositoriesApiDoc::openapi())
        .routes(routes!(list_public_repositories))
        .routes(routes!(get_public_repository))
        .routes(routes!(list_public_repo_tags))
        .routes(routes!(get_public_repo_tag))
}
