use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;

use dto_lib::ErrorResponse;

use crate::inbound::extractors::AuthApiKey;

/// Response body for POST /api/keys/validate
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct ValidateKeyResponse {
    pub valid: bool,
    pub message: String,
}

#[utoipa::path(
    post,
    path = "/validate",
    responses(
        (status = 200, description = "API key is valid", body = ValidateKeyResponse),
        (status = 401, description = "API key is invalid", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "keys"
)]
pub async fn validate_key(AuthApiKey(_key): AuthApiKey) -> impl IntoResponse {
    // If we reach this point, the AuthApiKey extractor has already validated the key
    // If the key was invalid, it would have returned 401 before reaching here
    (
        StatusCode::OK,
        Json(ValidateKeyResponse {
            valid: true,
            message: "API key is valid".to_string(),
        }),
    )
        .into_response()
}

#[derive(OpenApi)]
#[openapi(
    paths(validate_key),
    components(schemas(ValidateKeyResponse, ErrorResponse))
)]
pub struct KeysApiDoc;

pub fn keys_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(KeysApiDoc::openapi()).routes(utoipa_axum::routes!(validate_key))
}
