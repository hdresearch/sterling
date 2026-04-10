use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use dto_lib::{
    ErrorResponse,
    orchestrator::env_var::{EnvVarsResponse, SetEnvVarsRequest},
};

use crate::{
    action::{
        self, DeleteEnvVar, DeleteEnvVarError, ListEnvVars, ListEnvVarsError, SetEnvVars,
        SetEnvVarsError,
    },
    inbound::extractors::AuthApiKey,
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

#[utoipa::path(
    get,
    path = "",
    responses(
        (status = 200, description = "Current environment variables", body = EnvVarsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "env_vars"
)]
pub async fn list_env_vars(AuthApiKey(key): AuthApiKey) -> impl IntoResponse {
    match action_http!(action::call(ListEnvVars::new(key)).await) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => match err {
            ListEnvVarsError::Db(e) => ErrorResponse::internal_server_error(Some(e.to_string())),
        }
        .into_response(),
    }
}

#[utoipa::path(
    put,
    path = "",
    request_body = SetEnvVarsRequest,
    responses(
        (status = 200, description = "Environment variables updated", body = EnvVarsResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "env_vars"
)]
pub async fn set_env_vars(
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<SetEnvVarsRequest>,
) -> impl IntoResponse {
    match action_http!(action::call(SetEnvVars::new(key, req.vars, req.replace)).await) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(err) => match err {
            SetEnvVarsError::Validation(msg) => ErrorResponse::bad_request(Some(msg)),
            SetEnvVarsError::Db(e) => ErrorResponse::internal_server_error(Some(e.to_string())),
        }
        .into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/{key}",
    params(
        ("key" = String, Path, description = "Environment variable key to delete")
    ),
    responses(
        (status = 204, description = "Environment variable deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Environment variable not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "env_vars"
)]
pub async fn delete_env_var(
    AuthApiKey(key): AuthApiKey,
    Path(var_key): Path<String>,
) -> impl IntoResponse {
    match action_http!(action::call(DeleteEnvVar::new(key, var_key)).await) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => match err {
            DeleteEnvVarError::Db(e) => ErrorResponse::internal_server_error(Some(e.to_string())),
            DeleteEnvVarError::NotFound => {
                ErrorResponse::not_found(Some("Environment variable not found".to_string()))
            }
        }
        .into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(list_env_vars, set_env_vars, delete_env_var),
    components(schemas(SetEnvVarsRequest, EnvVarsResponse))
)]
pub struct EnvVarsApiDoc;

pub fn env_vars_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(EnvVarsApiDoc::openapi())
        .routes(routes!(list_env_vars))
        .routes(routes!(set_env_vars))
        .routes(routes!(delete_env_var))
}
