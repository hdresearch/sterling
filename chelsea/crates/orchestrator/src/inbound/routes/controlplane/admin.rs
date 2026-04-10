use axum::{
    Extension, Json, extract::Path, http::StatusCode, middleware::from_fn, response::IntoResponse,
};
use dto_lib::orchestrator::api_key::{GenerateApiKeyRequest, GenerateApiKeyResponse};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;
use vers_config::VersConfig;

use crate::{
    action::{self, GenerateApiKey, MoveVMError, SleepVMError, WakeVMError},
    inbound::{InboundState, middleware::check_admin_key},
};

/// Retrieve the current Vers configuration.
#[utoipa::path(
    get,
    path = "/config",
    responses(
        (status = 200, description = "Current configuration", body = VersConfig)
    )
)]
async fn get_config_handler() -> Json<&'static VersConfig> {
    let config = VersConfig::global();
    Json(config)
}

/// Generate a new API key for a given user and org.
#[utoipa::path(
    post,
    path = "/api_key",
    request_body = GenerateApiKeyRequest,
    responses(
        (status = 200, description = "API key created", body = GenerateApiKeyResponse),
        (status = 500, description = "Internal server error"),
    )
)]
async fn generate_api_key_handler(
    Extension(state): Extension<InboundState>,
    Json(req): Json<GenerateApiKeyRequest>,
) -> impl IntoResponse {
    match (GenerateApiKey {
        user_id: req.user_id,
        org_id: req.org_id,
        label: req.label,
    })
    .call(&state.db)
    .await
    {
        Ok(generated) => (
            StatusCode::OK,
            Json(GenerateApiKeyResponse {
                api_key: generated.api_key,
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "Failed to generate API key");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Request body for `POST /admin/vm/{vm_id}/sleep`.
#[derive(Serialize, Deserialize, ToSchema, Debug, Default)]
pub struct AdminSleepVmRequest {
    /// If true, error immediately rather than waiting for a booting VM. Default: false.
    pub skip_wait_boot: Option<bool>,
}

/// Request body for `POST /admin/vm/{vm_id}/wake`.
#[derive(Serialize, Deserialize, ToSchema, Debug, Default)]
pub struct AdminWakeVmRequest {
    /// The node to wake the VM on. If omitted, a node is chosen automatically.
    pub destination_node_id: Option<Uuid>,
}

/// Request body for `POST /admin/vm/{vm_id}/move`.
#[derive(Serialize, Deserialize, ToSchema, Debug, Default)]
pub struct AdminMoveVmRequest {
    /// The destination node. If omitted, a node is chosen automatically.
    pub destination_node_id: Option<Uuid>,
    /// If true, error immediately rather than waiting for a booting VM. Default: false.
    pub skip_wait_boot: Option<bool>,
}

/// Response body for wake and move operations, indicating where the VM was placed.
#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct AdminVmNodeResponse {
    /// The node the VM was woken on.
    pub node_id: Uuid,
}

/// Sleep a VM: snapshot it and kill its process. The VM can later be woken via the wake endpoint.
#[utoipa::path(
    post,
    path = "/vm/{vm_id}/sleep",
    params(("vm_id" = Uuid, Path, description = "VM ID to sleep")),
    request_body = AdminSleepVmRequest,
    responses(
        (status = 200, description = "VM is now sleeping"),
        (status = 404, description = "VM not found"),
        (status = 409, description = "VM is already sleeping"),
        (status = 500, description = "Internal server error"),
    )
)]
async fn admin_sleep_vm_handler(
    Path(vm_id): Path<Uuid>,
    Json(req): Json<AdminSleepVmRequest>,
) -> impl IntoResponse {
    let skip_wait_boot = req.skip_wait_boot.unwrap_or(false);
    match action::call(action::SleepVM::new(vm_id, skip_wait_boot)).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            if let Some(err) = e.try_extract_err() {
                match err {
                    SleepVMError::VmNotFound => StatusCode::NOT_FOUND.into_response(),
                    SleepVMError::VmAlreadySleeping => StatusCode::CONFLICT.into_response(),
                    SleepVMError::NodeNotFound => {
                        tracing::error!(vm_id = %vm_id, "Node not found while sleeping VM");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                    SleepVMError::Db(e) => {
                        tracing::error!(vm_id = %vm_id, error = ?e, "DB error sleeping VM");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                    SleepVMError::Http(e) => {
                        tracing::error!(vm_id = %vm_id, error = ?e, "HTTP error sleeping VM");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                }
            } else {
                tracing::error!(vm_id = %vm_id, "Unexpected error sleeping VM");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

/// Wake a sleeping VM, optionally on a specific destination node.
///
/// If `destination_node_id` is omitted, a node is selected automatically.
#[utoipa::path(
    post,
    path = "/vm/{vm_id}/wake",
    params(("vm_id" = Uuid, Path, description = "VM ID to wake")),
    request_body = AdminWakeVmRequest,
    responses(
        (status = 200, description = "VM woken successfully", body = AdminVmNodeResponse),
        (status = 404, description = "VM not found or destination node not found"),
        (status = 409, description = "VM is not sleeping"),
        (status = 500, description = "Internal server error"),
    )
)]
async fn admin_wake_vm_handler(
    Path(vm_id): Path<Uuid>,
    Json(req): Json<AdminWakeVmRequest>,
) -> impl IntoResponse {
    match action::call(action::WakeVM::new(vm_id, req.destination_node_id)).await {
        Ok(node_id) => (StatusCode::OK, Json(AdminVmNodeResponse { node_id })).into_response(),
        Err(e) => {
            if let Some(err) = e.try_extract_err() {
                match err {
                    WakeVMError::VmNotFound | WakeVMError::NodeNotFound => {
                        StatusCode::NOT_FOUND.into_response()
                    }
                    WakeVMError::VmNotSleeping => StatusCode::CONFLICT.into_response(),
                    WakeVMError::NoAvailableNodes(e) => {
                        tracing::error!(vm_id = %vm_id, error = ?e, "No available nodes for wake");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                    WakeVMError::Db(e) => {
                        tracing::error!(vm_id = %vm_id, error = ?e, "DB error waking VM");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                    WakeVMError::Http(e) => {
                        tracing::error!(vm_id = %vm_id, error = ?e, "HTTP error waking VM");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                    WakeVMError::InternalServerError => {
                        tracing::error!(vm_id = %vm_id, "Internal error waking VM");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                }
            } else {
                tracing::error!(vm_id = %vm_id, "Unexpected error waking VM");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

/// Move a VM to a new node by sleeping it on the current node and waking it on the destination.
///
/// If `destination_node_id` is omitted, a node is selected automatically.
#[utoipa::path(
    post,
    path = "/vm/{vm_id}/move",
    params(("vm_id" = Uuid, Path, description = "VM ID to move")),
    request_body = AdminMoveVmRequest,
    responses(
        (status = 200, description = "VM moved successfully", body = AdminVmNodeResponse),
        (status = 404, description = "VM not found or destination node not found"),
        (status = 409, description = "VM is already sleeping"),
        (status = 500, description = "Internal server error"),
    )
)]
async fn admin_move_vm_handler(
    Path(vm_id): Path<Uuid>,
    Json(req): Json<AdminMoveVmRequest>,
) -> impl IntoResponse {
    let skip_wait_boot = req.skip_wait_boot.unwrap_or(false);
    match action::call(action::MoveVM::new(
        vm_id,
        req.destination_node_id,
        skip_wait_boot,
    ))
    .await
    {
        Ok(node_id) => (StatusCode::OK, Json(AdminVmNodeResponse { node_id })).into_response(),
        Err(e) => {
            if let Some(err) = e.try_extract_err() {
                match err {
                    MoveVMError::Sleep(SleepVMError::VmNotFound) => {
                        StatusCode::NOT_FOUND.into_response()
                    }
                    MoveVMError::Sleep(SleepVMError::VmAlreadySleeping) => {
                        StatusCode::CONFLICT.into_response()
                    }
                    MoveVMError::Wake(WakeVMError::NodeNotFound) => {
                        StatusCode::NOT_FOUND.into_response()
                    }
                    MoveVMError::Sleep(e) => {
                        tracing::error!(vm_id = %vm_id, error = ?e, "Sleep phase failed during move");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                    MoveVMError::Wake(e) => {
                        tracing::error!(vm_id = %vm_id, error = ?e, "Wake phase failed during move");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                    MoveVMError::InternalServerError => {
                        tracing::error!(vm_id = %vm_id, "Internal error during move");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                }
            } else {
                tracing::error!(vm_id = %vm_id, "Unexpected error moving VM");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

pub fn admin_routes() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_config_handler))
        .routes(routes!(generate_api_key_handler))
        .routes(routes!(admin_sleep_vm_handler))
        .routes(routes!(admin_wake_vm_handler))
        .routes(routes!(admin_move_vm_handler))
        .layer(from_fn(check_admin_key))
}
