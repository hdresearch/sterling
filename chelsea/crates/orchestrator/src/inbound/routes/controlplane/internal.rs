use axum::{Extension, extract::Path, middleware, response::IntoResponse, routing::post};
use reqwest::StatusCode;
use uuid::Uuid;

use utoipa_axum::router::OpenApiRouter;

use crate::{
    action::MarkVmBootFailed,
    inbound::{InboundState, middleware::check_admin_key},
};

/// POST /internal/vm/:vm_id/boot-failed
///
/// Called by Chelsea nodes when a VM fails to boot and has been cleaned up locally.
/// This allows Chelsea to proactively notify the orchestrator so the DB record
/// is marked as deleted immediately, rather than waiting for the reconciliation loop.
///
/// Protected by the admin API key (same as node management endpoints).
async fn vm_boot_failed(
    Extension(state): Extension<InboundState>,
    Path(vm_id): Path<Uuid>,
) -> impl IntoResponse {
    tracing::info!(%vm_id, "Received boot-failed callback from Chelsea");

    match MarkVmBootFailed::new(vm_id).call(&state.db).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            tracing::error!(%vm_id, ?e, "Failed to mark VM as boot-failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub fn internal_routes() -> OpenApiRouter {
    OpenApiRouter::new()
        .route("/vm/{vm_id}/boot-failed", post(vm_boot_failed))
        .layer(middleware::from_fn(check_admin_key))
}
