use axum::{Json, http::StatusCode, response::IntoResponse};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use dto_lib::ErrorResponse;

use crate::{
    action::{self, DeployFromGitHub, DeployRequest, DeployResponse},
    inbound::{OperationId, extractors::AuthApiKey},
};

/// Deploy a GitHub repository to a new Vers project.
#[utoipa::path(
    post,
    path = "",
    request_body = DeployRequest,
    responses(
        (status = 202, description = "Deploy initiated", body = DeployResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 409, description = "Project name conflict", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
        (status = 501, description = "GitHub App not configured", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "deploy"
)]
pub async fn deploy_handler(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<DeployRequest>,
) -> impl IntoResponse {
    match action::call(
        DeployFromGitHub::new(req, key).with_request_id(Some(operation_id.as_str().to_string())),
    )
    .await
    {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(action_err) => match action_err.try_extract_err() {
            Some(e) => e.into_response(),
            None => ErrorResponse::internal_server_error(None).into_response(),
        },
    }
}

pub fn deploy_routes() -> OpenApiRouter {
    OpenApiRouter::new().routes(routes!(deploy_handler))
}
