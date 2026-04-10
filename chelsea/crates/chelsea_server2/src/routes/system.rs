use std::sync::Arc;

use crate::types::error::ChelseaServerError;
use crate::{ChelseaServerCore, utils::get_all_versions};
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse};
use dto_lib::chelsea_server2::system::{ChelseaVersion, SystemTelemetryResponse};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/api/system/health",
    responses(
        (status = 200, description = "Health check successful", body = String)
    )
)]
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// Telemetry endpoint (return type to be filled in)
#[utoipa::path(
    get,
    path = "/api/system/telemetry",
    responses(
        (status = 200, description = "Telemetry information returned", body = SystemTelemetryResponse),
        (status = 500, description = "Internal error while retrieving system info", body = ChelseaServerError)
    )
)]
async fn telemetry_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
) -> Result<(StatusCode, Json<SystemTelemetryResponse>), ChelseaServerError> {
    let ret = core.get_system_telemetry().await?;
    Ok((StatusCode::OK, Json(ret)))
}

/// Version endpoint
#[utoipa::path(
    get,
    path = "/api/system/version",
    responses(
        (status = 200, description = "Current build version information", body = ChelseaVersion),
        (status = 500, description = "Internal error while retrieving version info", body = String)
    )
)]
async fn version_handler() -> impl IntoResponse {
    match get_all_versions().await {
        Ok(versions) => (StatusCode::OK, Json(versions)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(health_handler, telemetry_handler, version_handler),
    components(schemas(SystemTelemetryResponse, ChelseaServerError, ChelseaVersion))
)]
pub struct SystemApiDoc;

/// Create the system router
pub fn create_system_router(
    core: Arc<dyn ChelseaServerCore>,
) -> (Router, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(SystemApiDoc::openapi())
        .routes(routes!(health_handler))
        .routes(routes!(telemetry_handler))
        .routes(routes!(version_handler))
        .with_state(core)
        .split_for_parts()
}
