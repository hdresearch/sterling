use axum::Json;
use dto_lib::orchestrator::system::OrchestratorVersion;
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

#[utoipa::path(
    get,
    path = "/version",
    responses(
        (status = 200, description = "Version information", body = OrchestratorVersion)
    )
)]
async fn version_handler() -> Json<OrchestratorVersion> {
    let workspace_version = workspace_build::workspace_version().to_string();
    let git_hash = workspace_build::git_hash().to_string();

    Json(OrchestratorVersion {
        executable_name: "orchestrator".to_string(),
        workspace_version,
        git_hash,
    })
}

#[derive(OpenApi)]
#[openapi(paths(version_handler))]
pub struct SystemApiDoc;

pub fn system_router() -> OpenApiRouter {
    OpenApiRouter::with_openapi(SystemApiDoc::openapi()).routes(routes!(version_handler))
}
