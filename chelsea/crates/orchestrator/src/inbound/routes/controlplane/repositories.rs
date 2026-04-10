use axum::{Extension, Json, extract::Path, http::StatusCode, response::IntoResponse};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use dto_lib::{
    ErrorResponse,
    orchestrator::commit_repository::{
        CreateRepoTagRequest, CreateRepoTagResponse, CreateRepositoryRequest,
        CreateRepositoryResponse, ForkRepositoryRequest, ForkRepositoryResponse,
        ListRepoTagsResponse, ListRepositoriesResponse, RepoTagInfo, RepositoryInfo,
        SetRepositoryVisibilityRequest, UpdateRepoTagRequest,
    },
};

use crate::{
    action::{
        self, CreateRepoTag, CreateRepository, DeleteRepoTag, DeleteRepository, ForkRepository,
        GetRepoTag, GetRepository, ListRepoTags, ListRepositories, SetRepositoryVisibility,
        UpdateRepoTag,
    },
    inbound::{InboundState, extractors::AuthApiKey},
};

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

// ── Repository CRUD ─────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "",
    request_body = CreateRepositoryRequest,
    responses(
        (status = 201, description = "Repository created", body = CreateRepositoryResponse),
        (status = 400, description = "Invalid name", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Repository already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn create_repository(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<CreateRepositoryRequest>,
) -> impl IntoResponse {
    match CreateRepository::new(req.name, req.description, key)
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
        (status = 200, description = "List of repositories", body = ListRepositoriesResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn list_repositories(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
) -> impl IntoResponse {
    match ListRepositories::new(key).call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{repo_name}",
    params(("repo_name" = String, Path, description = "Repository name")),
    responses(
        (status = 200, description = "Repository details", body = RepositoryInfo),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn get_repository(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(repo_name): Path<String>,
) -> impl IntoResponse {
    match GetRepository::new(repo_name, key).call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/{repo_name}",
    params(("repo_name" = String, Path, description = "Repository name")),
    responses(
        (status = 204, description = "Repository deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn delete_repository(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(repo_name): Path<String>,
) -> impl IntoResponse {
    match DeleteRepository::new(repo_name, key).call(&state.db).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

// ── Repository Tag CRUD ─────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/{repo_name}/tags",
    request_body = CreateRepoTagRequest,
    params(("repo_name" = String, Path, description = "Repository name")),
    responses(
        (status = 201, description = "Tag created", body = CreateRepoTagResponse),
        (status = 400, description = "Invalid tag name", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Repository or commit not found", body = ErrorResponse),
        (status = 409, description = "Tag already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn create_repo_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(repo_name): Path<String>,
    Json(req): Json<CreateRepoTagRequest>,
) -> impl IntoResponse {
    match CreateRepoTag::new(repo_name, req.tag_name, req.commit_id, req.description, key)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{repo_name}/tags",
    params(("repo_name" = String, Path, description = "Repository name")),
    responses(
        (status = 200, description = "List of tags in repository", body = ListRepoTagsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn list_repo_tags(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(repo_name): Path<String>,
) -> impl IntoResponse {
    match ListRepoTags::new(repo_name, key).call(&state.db).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{repo_name}/tags/{tag_name}",
    params(
        ("repo_name" = String, Path, description = "Repository name"),
        ("tag_name" = String, Path, description = "Tag name"),
    ),
    responses(
        (status = 200, description = "Tag details", body = RepoTagInfo),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Repository or tag not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn get_repo_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path((repo_name, tag_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match GetRepoTag::new(repo_name, tag_name, key)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/{repo_name}/tags/{tag_name}",
    request_body = UpdateRepoTagRequest,
    params(
        ("repo_name" = String, Path, description = "Repository name"),
        ("tag_name" = String, Path, description = "Tag name"),
    ),
    responses(
        (status = 204, description = "Tag updated"),
        (status = 400, description = "No updates provided", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Repository, tag, or commit not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn update_repo_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path((repo_name, tag_name)): Path<(String, String)>,
    Json(req): Json<UpdateRepoTagRequest>,
) -> impl IntoResponse {
    match UpdateRepoTag::new(repo_name, tag_name, req.commit_id, req.description, key)
        .call(&state.db)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/{repo_name}/tags/{tag_name}",
    params(
        ("repo_name" = String, Path, description = "Repository name"),
        ("tag_name" = String, Path, description = "Tag name"),
    ),
    responses(
        (status = 204, description = "Tag deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Repository or tag not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn delete_repo_tag(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path((repo_name, tag_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match DeleteRepoTag::new(repo_name, tag_name, key)
        .call(&state.db)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

// ── Repository Visibility ────────────────────────────────────────────────

#[utoipa::path(
    patch,
    path = "/{repo_name}/visibility",
    request_body = SetRepositoryVisibilityRequest,
    params(("repo_name" = String, Path, description = "Repository name")),
    responses(
        (status = 204, description = "Visibility updated"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn set_repository_visibility(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(repo_name): Path<String>,
    Json(req): Json<SetRepositoryVisibilityRequest>,
) -> impl IntoResponse {
    match SetRepositoryVisibility::new(repo_name, req.is_public, key)
        .call(&state.db)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

// ── Fork ────────────────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/fork",
    request_body = ForkRepositoryRequest,
    responses(
        (status = 201, description = "Repository forked successfully", body = ForkRepositoryResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Source repository or tag not found", body = ErrorResponse),
        (status = 409, description = "Target repository already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "repositories"
)]
pub async fn fork_repository(
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<ForkRepositoryRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(ForkRepository::new(
            req.source_org,
            req.source_repo,
            req.source_tag,
            req.repo_name,
            req.tag_name,
            key,
        ))
        .await
    ) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── OpenAPI + Router ────────────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
    paths(
        create_repository,
        list_repositories,
        get_repository,
        delete_repository,
        set_repository_visibility,
        fork_repository,
        create_repo_tag,
        list_repo_tags,
        get_repo_tag,
        update_repo_tag,
        delete_repo_tag,
    ),
    components(schemas(
        CreateRepositoryRequest,
        CreateRepositoryResponse,
        RepositoryInfo,
        ListRepositoriesResponse,
        SetRepositoryVisibilityRequest,
        ForkRepositoryRequest,
        ForkRepositoryResponse,
        CreateRepoTagRequest,
        CreateRepoTagResponse,
        RepoTagInfo,
        ListRepoTagsResponse,
        UpdateRepoTagRequest,
    ))
)]
pub struct RepositoriesApiDoc;

pub fn repositories_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(RepositoriesApiDoc::openapi())
        .routes(routes!(create_repository))
        .routes(routes!(list_repositories))
        .routes(routes!(fork_repository))
        .routes(routes!(get_repository))
        .routes(routes!(delete_repository))
        .routes(routes!(set_repository_visibility))
        .routes(routes!(create_repo_tag))
        .routes(routes!(list_repo_tags))
        .routes(routes!(get_repo_tag))
        .routes(routes!(update_repo_tag))
        .routes(routes!(delete_repo_tag))
}
