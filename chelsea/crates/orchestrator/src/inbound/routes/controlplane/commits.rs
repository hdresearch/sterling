use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use dto_lib::chelsea_server2::commits::{
    CommitInfo, ListCommitsQuery, ListCommitsResponse, ListPublicCommitsQuery, UpdateCommitRequest,
};

use crate::{
    action::{DeleteCommit, ListCommits, SetCommitPublic},
    inbound::{InboundState, extractors::AuthApiKey},
};

#[utoipa::path(
    get,
    path = "",
    params(ListCommitsQuery),
    responses(
        (status = 200, description = "List of commits", body = ListCommitsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commits"
)]
async fn list_commits(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Query(query): Query<ListCommitsQuery>,
) -> impl IntoResponse {
    match ListCommits::new(key, query.limit, query.offset)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/public",
    params(ListPublicCommitsQuery),
    responses(
        (status = 200, description = "List of public commits", body = ListCommitsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commits"
)]
async fn list_public_commits(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Query(query): Query<ListPublicCommitsQuery>,
) -> impl IntoResponse {
    match ListCommits::public(key, query.limit, query.offset)
        .call(&state.db)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/{commit_id}",
    params(("commit_id" = Uuid, Path, description = "The commit ID")),
    request_body = UpdateCommitRequest,
    responses(
        (status = 200, description = "Commit updated", body = CommitInfo),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Commit not found", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commits"
)]
async fn update_commit(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(commit_id): Path<Uuid>,
    Json(req): Json<UpdateCommitRequest>,
) -> impl IntoResponse {
    let name = req.name.filter(|s| !s.trim().is_empty());
    let description = req.description.filter(|s| !s.trim().is_empty());

    match SetCommitPublic::new(commit_id, req.is_public, key)
        .with_name(name)
        .with_description(description)
        .call(&state.db)
        .await
    {
        Ok(commit) => (StatusCode::OK, Json(CommitInfo::from(commit))).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/{commit_id}",
    params(("commit_id" = Uuid, Path, description = "Commit ID to delete")),
    responses(
        (status = 204, description = "Commit deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = dto_lib::ErrorResponse),
        (status = 404, description = "Commit not found", body = dto_lib::ErrorResponse),
        (status = 409, description = "Commit in use", body = dto_lib::ErrorResponse),
        (status = 500, description = "Internal server error", body = dto_lib::ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commits"
)]
async fn delete_commit(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(commit_id): Path<Uuid>,
) -> impl IntoResponse {
    match DeleteCommit::new(key, commit_id)
        .call(&state.db, &state.vers_pg)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(list_commits, list_public_commits, update_commit, delete_commit),
    components(schemas(
        ListCommitsResponse,
        CommitInfo,
        UpdateCommitRequest,
        dto_lib::ErrorResponse,
    ))
)]
pub struct CommitsApiDoc;

pub fn commits_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(CommitsApiDoc::openapi())
        .routes(routes!(list_commits))
        .routes(routes!(list_public_commits))
        .routes(routes!(update_commit, delete_commit))
}
