use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose};
use bytes::Bytes;
use chelsea_lib::vsock::{ExecStreamConnection, ExecStreamEvent};
use dto_lib::chelsea_server2::{error::ChelseaServerError, vm::*};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::error;
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{ChelseaServerCore, routes::util::spawn_detached};

/// Commit the current state of a VM
#[utoipa::path(
    post,
    path = "/api/vm/{vm_id}/commit",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID) to commit"),
        ("keep_paused" = Option<bool>, Query, description = "If set to true, the VM will remain paused after commit"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    request_body(
        content = VmCommitRequest,
        description = "The commit ID to use for commit file uploads, etc."
    ),
    responses(
        (status = 200, description = "Successfully committed VM state", body = VmCommitResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_commit_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmCommitQuery>,
    Json(request): Json<VmCommitRequest>,
) -> Result<Json<VmCommitResponse>, ChelseaServerError> {
    match spawn_detached({
        let vm_id = vm_id.clone();
        let commit_id = request.commit_id.unwrap_or_else(Uuid::new_v4);
        async move {
            core.vm_commit(
                &vm_id,
                commit_id,
                query.keep_paused.unwrap_or(false),
                !query.skip_wait_boot.unwrap_or(false),
            )
            .await
        }
    })
    .await
    {
        Ok(response) => Ok(Json(response)),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/commit");
            Err(error.into())
        }
    }
}

/// Delete a VM
#[utoipa::path(
    delete,
    path = "/api/vm/{vm_id}",
    description = "Delete the specified VM.",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID) to delete"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    responses(
        (status = 200, description = "Successfully deleted VM"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_delete_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmDeleteQuery>,
) -> Result<StatusCode, ChelseaServerError> {
    match spawn_detached({
        let vm_id = vm_id.clone();
        async move {
            core.vm_delete(&vm_id, !query.skip_wait_boot.unwrap_or(false))
                .await
        }
    })
    .await
    {
        Ok(()) => Ok(StatusCode::OK),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id} DELETE");
            Err(error.into())
        }
    }
}

/// List all VMs
#[utoipa::path(
    get,
    path = "/api/vm",
    responses(
        (status = 200, description = "Successfully retrieved all VMs", body = VmListAllResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_list_all_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
) -> Result<Json<VmListAllResponse>, ChelseaServerError> {
    match core.vm_list_all().await {
        Ok(response) => Ok(Json(response)),
        Err(error) => {
            error!(?error, "Error on /api/vm GET");
            Err(error.into())
        }
    }
}

/// Get the status of a VM
#[utoipa::path(
    get,
    path = "/api/vm/{vm_id}",
    params(
        ("vm_id" = Uuid, Path, description = "The VM ID (v4 UUID) whose status to fetch")
    ),
    responses(
        (status = 200, description = "Successfully fetched VM status", body = VmStatusResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_status_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<VmStatusResponse>, ChelseaServerError> {
    match core.vm_status(&vm_id).await {
        Ok(status) => Ok(Json(status)),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/status GET");
            Err(error.into())
        }
    }
}

/// Create a new VM
#[utoipa::path(
    post,
    path = "/api/vm/new",
    params(
        ("wait_boot" = Option<bool>, Query, description = "If true, wait for the VM to finish booting before returning")
    ),
    request_body = VmCreateRequest,
    responses(
        (status = 200, description = "Successfully created new root VM", body = VmCreateResponse),
        (status = 500, description = "Internal server error")
    )
)]
#[tracing::instrument(skip_all, name = "vm_create_handler")]
pub async fn vm_create_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Query(query): Query<VmCreateQuery>,
    Json(request): Json<VmCreateRequest>,
) -> Result<Json<VmCreateResponse>, ChelseaServerError> {
    match spawn_detached(async move {
        core.vm_create(request, query.wait_boot.unwrap_or(false))
            .await
    })
    .await
    {
        Ok(vm_id) => Ok(Json(VmCreateResponse { vm_id })),
        Err(error) => {
            error!(?error, "Error on /api/vm/new_root POST");
            Err(error)
        }
    }
}

/// Update the state of a VM
#[utoipa::path(
    patch,
    path = "/api/vm/{vm_id}/state",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID) to update"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Defaults to false.")
    ),
    request_body = VmUpdateStateRequest,
    responses(
        (status = 200, description = "Successfully updated VM state"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_update_state_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmUpdateStateQuery>,
    Json(request): Json<VmUpdateStateRequest>,
) -> Result<StatusCode, ChelseaServerError> {
    let core_clone = core.clone();
    let vm_id_owned = vm_id.clone();
    spawn_detached(async move {
        match core_clone
            .vm_update_state(
                &vm_id_owned,
                request,
                !query.skip_wait_boot.unwrap_or(false),
            )
            .await
        {
            Ok(()) => Ok(StatusCode::OK),
            Err(error) => {
                error!(?error, "Error on /api/vm/{vm_id}/state PATCH");
                Err(ChelseaServerError::from(error))
            }
        }
    })
    .await
}

/// Create a new VM from a commit
#[utoipa::path(
    post,
    path = "/api/vm/from_commit",
    request_body = VmFromCommitRequest,
    responses(
        (status = 200, description = "Successfully created VM from commit", body = VmFromCommitResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_from_commit_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Json(request): Json<VmFromCommitRequest>,
) -> Result<Json<VmFromCommitResponse>, ChelseaServerError> {
    let core_clone = core.clone();
    match spawn_detached(async move { core_clone.vm_from_commit(request).await }).await {
        Ok(vm_id) => Ok(Json(VmFromCommitResponse { vm_id })),
        Err(error) => {
            error!(?error, "Error on /api/vm/from_commit POST");
            Err(error)
        }
    }
}

/// Get the SSH public key for a VM
#[utoipa::path(
    get,
    path = "/api/vm/{vm_id}/ssh_key",
    params(
        ("vm_id" = Uuid, Path, description = "The VM ID (v4 UUID) to retrieve the SSH key for")
    ),
    responses(
        (status = 200, description = "Successfully retrieved VM SSH public key", body = VmSshKeyResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_get_ssh_key_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
) -> Result<Json<VmSshKeyResponse>, ChelseaServerError> {
    match core.vm_get_ssh_key_and_port(&vm_id).await {
        Ok((ssh_private_key, ssh_port)) => Ok(Json(VmSshKeyResponse {
            ssh_private_key,
            ssh_port,
        })),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/ssh_key GET");
            Err(error.into())
        }
    }
}

/// Resize a VM's disk. The VM is briefly paused while the underlying volume is grown, then automatically resumed.
/// Only growing is supported; the new size must be strictly greater than the current size.
#[utoipa::path(
    patch,
    path = "/api/vm/{vm_id}/disk",
    params(
        ("vm_id" = Uuid, Path, description = "The VM ID (v4 UUID) whose disk to resize"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    request_body = VmResizeDiskRequest,
    responses(
        (status = 200, description = "Successfully resized VM disk"),
        (status = 400, description = "Invalid resize request (e.g. new size not larger than current)"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_resize_disk_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmResizeDiskQuery>,
    Json(request): Json<VmResizeDiskRequest>,
) -> Result<StatusCode, ChelseaServerError> {
    let core_clone = core.clone();
    let vm_id_owned = vm_id.clone();
    spawn_detached(async move {
        match core_clone
            .vm_resize_disk(
                &vm_id_owned,
                request,
                !query.skip_wait_boot.unwrap_or(false),
            )
            .await
        {
            Ok(()) => Ok(StatusCode::OK),
            Err(error) => {
                error!(?error, "Error on /api/vm/{vm_id}/disk PATCH");
                Err(ChelseaServerError::from(error))
            }
        }
    })
    .await
}

/// Put a VM to sleep. This snapshots the VM, then kills the associated process, allowing it to be later resumed via the wake command.
#[utoipa::path(
    post,
    path = "/api/vm/{vm_id}/sleep",
    params(
        ("vm_id" = Uuid, Path, description = "The VM ID (v4 UUID) to put to sleep"),
        ("skip_wait_boot" = bool, Query, description = "If set to true, error immediately rather than waiting for a booting VM (default: false)")
    ),
    responses(
        (status = 200, description = "VM successfully put to sleep"),
        (status = 404, description = "VM not found"),
        (status = 409, description = "VM cannot be put to sleep from current state"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_sleep_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmSleepQuery>,
) -> Result<(), ChelseaServerError> {
    let core_clone = core.clone();
    let wait_boot = !query.skip_wait_boot.unwrap_or(false);
    match spawn_detached(async move { core_clone.vm_sleep(&vm_id, wait_boot).await }).await {
        Ok(()) => Ok(()),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/sleep POST");
            Err(error.into())
        }
    }
}

/// Wake a sleeping VM. This starts a VM from the temporary snapshot associated with it, then deletes that snapshot.
#[utoipa::path(
    post,
    path = "/api/vm/{vm_id}/wake",
    request_body = VmWakeRequest,
    params(
        ("vm_id" = Uuid, Path, description = "The VM ID (v4 UUID) to wake"),
    ),
    responses(
        (status = 200, description = "VM successfully woken"),
        (status = 404, description = "VM not found"),
        (status = 409, description = "VM cannot be woken from current state"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_wake_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Json(request): Json<VmWakeRequest>,
) -> Result<(), ChelseaServerError> {
    let core_clone = core.clone();
    match spawn_detached(async move { core_clone.vm_wake(&vm_id, request).await }).await {
        Ok(()) => Ok(()),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/wake POST");
            Err(error.into())
        }
    }
}

/// Execute a command inside a VM
#[utoipa::path(
    post,
    path = "/api/vm/{vm_id}/exec",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID) to run the command in"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    request_body = VmExecRequest,
    responses(
        (status = 200, description = "Command completed successfully", body = VmExecResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_exec_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmExecQuery>,
    Json(request): Json<VmExecRequest>,
) -> Result<Json<VmExecResponse>, ChelseaServerError> {
    match core
        .vm_exec(&vm_id, request, !query.skip_wait_boot.unwrap_or(false))
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/exec POST");
            Err(error.into())
        }
    }
}

/// Stream stdout/stderr chunks inside a VM (NDJSON)
#[utoipa::path(
    post,
    path = "/api/vm/{vm_id}/exec/stream",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID) to run the command in"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    request_body = VmExecRequest,
    responses(
        (status = 200, description = "NDJSON stream with stdout/stderr chunks", body = VmExecStreamEvent),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_exec_stream_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmExecQuery>,
    Json(request): Json<VmExecRequest>,
) -> Result<Response, ChelseaServerError> {
    let wait_boot = !query.skip_wait_boot.unwrap_or(false);
    let connection = core.vm_exec_stream(&vm_id, request, wait_boot).await?;

    let (tx, rx) = mpsc::channel::<Bytes>(32);
    tokio::spawn(async move {
        if let Err(error) = forward_exec_stream(connection, tx).await {
            error!(?error, "Exec stream terminated with error");
        }
    });

    let stream = ReceiverStream::new(rx).map(Ok::<Bytes, axum::Error>);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    Ok(response)
}

/// Reattach to a running exec stream (NDJSON)
#[utoipa::path(
    post,
    path = "/api/vm/{vm_id}/exec/stream/attach",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID)"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    request_body = VmExecStreamAttachRequest,
    responses(
        (status = 200, description = "NDJSON stream replaying output since the requested cursor", body = VmExecStreamEvent),
        (status = 404, description = "Exec session not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_exec_stream_attach_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmExecQuery>,
    Json(request): Json<VmExecStreamAttachRequest>,
) -> Result<Response, ChelseaServerError> {
    let wait_boot = !query.skip_wait_boot.unwrap_or(false);
    let connection = core
        .vm_exec_stream_attach(&vm_id, request, wait_boot)
        .await?;

    let (tx, rx) = mpsc::channel::<Bytes>(32);
    tokio::spawn(async move {
        if let Err(error) = forward_exec_stream(connection, tx).await {
            error!(?error, "Exec stream attach terminated with error");
        }
    });

    let stream = ReceiverStream::new(rx).map(Ok::<Bytes, axum::Error>);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    Ok(response)
}

/// Retrieve exec logs for a VM
#[utoipa::path(
    get,
    path = "/api/vm/{vm_id}/exec/logs",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID) to fetch logs for"),
        ("offset" = Option<u64>, Query, description = "Byte offset into the log file (default: 0)"),
        ("max_entries" = Option<u32>, Query, description = "Maximum number of log entries to return"),
        ("stream" = Option<VmExecLogStream>, Query, description = "Filter by stdout/stderr"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if VM is still booting")
    ),
    responses(
        (status = 200, description = "Exec logs retrieved", body = VmExecLogResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_exec_logs_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmExecLogQuery>,
) -> Result<Json<VmExecLogResponse>, ChelseaServerError> {
    let wait_boot = !query.skip_wait_boot.unwrap_or(false);
    match core.vm_exec_logs(&vm_id, query, wait_boot).await {
        Ok(response) => Ok(Json(response)),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/exec/logs GET");
            Err(error.into())
        }
    }
}

// ── File Transfer ────────────────────────────────────────────────────

/// Write a file into a VM
#[utoipa::path(
    put,
    path = "/api/vm/{vm_id}/files",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID)"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    request_body = VmWriteFileRequest,
    responses(
        (status = 200, description = "File written successfully"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_write_file_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmExecQuery>,
    Json(request): Json<VmWriteFileRequest>,
) -> Result<StatusCode, ChelseaServerError> {
    match core
        .vm_write_file(&vm_id, request, !query.skip_wait_boot.unwrap_or(false))
        .await
    {
        Ok(()) => Ok(StatusCode::OK),
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/files PUT");
            Err(error.into())
        }
    }
}

/// Read a file from a VM
#[utoipa::path(
    get,
    path = "/api/vm/{vm_id}/files",
    params(
        ("vm_id" = String, Path, description = "The VM ID (v4 UUID)"),
        ("path" = String, Query, description = "Absolute path of the file to read"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false.")
    ),
    responses(
        (status = 200, description = "File contents", body = VmReadFileResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn vm_read_file_handler(
    State(core): State<Arc<dyn ChelseaServerCore>>,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmReadFileQuery>,
) -> Result<Json<VmReadFileResponse>, ChelseaServerError> {
    match core
        .vm_read_file(&vm_id, &query.path, !query.skip_wait_boot.unwrap_or(false))
        .await
    {
        Ok(content) => {
            let content_b64 = general_purpose::STANDARD.encode(&content);
            Ok(Json(VmReadFileResponse { content_b64 }))
        }
        Err(error) => {
            error!(?error, "Error on /api/vm/{vm_id}/files GET");
            Err(error.into())
        }
    }
}

/// Query params for GET /api/vm/{vm_id}/files
#[derive(Debug, serde::Deserialize)]
pub struct VmReadFileQuery {
    pub path: String,
    pub skip_wait_boot: Option<bool>,
}

/// Forward events from an [`ExecStreamConnection`] to an MPSC channel as
/// NDJSON-encoded [`VmExecStreamEvent`] lines.
async fn forward_exec_stream(
    mut connection: ExecStreamConnection,
    tx: mpsc::Sender<Bytes>,
) -> Result<(), chelsea_lib::vsock::VsockError> {
    loop {
        match connection.next_event().await? {
            Some(ExecStreamEvent::Chunk(chunk)) => {
                let stream = match chunk.stream {
                    agent_protocol::ExecLogStream::Stdout => VmExecLogStream::Stdout,
                    agent_protocol::ExecLogStream::Stderr => VmExecLogStream::Stderr,
                };
                let event = VmExecStreamEvent::Chunk {
                    exec_id: chunk.exec_id,
                    cursor: chunk.cursor,
                    stream,
                    data_b64: general_purpose::STANDARD.encode(&chunk.data),
                };
                let mut line = serde_json::to_vec(&event).unwrap_or_else(|_| b"{}".to_vec());
                line.push(b'\n');
                if tx.send(Bytes::from(line)).await.is_err() {
                    break; // client disconnected
                }
            }
            Some(ExecStreamEvent::Exit(exit)) => {
                let event = VmExecStreamEvent::Exit {
                    exec_id: exit.exec_id,
                    cursor: exit.cursor,
                    exit_code: exit.exit_code,
                };
                let mut line = serde_json::to_vec(&event).unwrap_or_else(|_| b"{}".to_vec());
                line.push(b'\n');
                let _ = tx.send(Bytes::from(line)).await;
                break;
            }
            None => break, // EOF
        }
    }
    Ok(())
}

// OpenAPI documentation structure
#[derive(OpenApi)]
#[openapi(paths(
    vm_commit_handler,
    vm_delete_handler,
    vm_list_all_handler,
    vm_create_handler,
    vm_update_state_handler,
    vm_exec_handler,
    vm_exec_stream_handler,
    vm_exec_stream_attach_handler,
    vm_exec_logs_handler,
    vm_from_commit_handler,
    vm_get_ssh_key_handler,
    vm_resize_disk_handler,
    vm_sleep_handler,
    vm_wake_handler,
    vm_status_handler,
    vm_write_file_handler,
    vm_read_file_handler
))]
pub struct VmApiDoc;

/// Create an exec-only router for testing.
#[cfg(test)]
pub fn create_exec_router(core: Arc<dyn ChelseaServerCore>) -> Router {
    Router::new()
        .route("/api/vm/{vm_id}/exec", axum::routing::post(vm_exec_handler))
        .route(
            "/api/vm/{vm_id}/exec/stream",
            axum::routing::post(vm_exec_stream_handler),
        )
        .route(
            "/api/vm/{vm_id}/exec/stream/attach",
            axum::routing::post(vm_exec_stream_attach_handler),
        )
        .route(
            "/api/vm/{vm_id}/exec/logs",
            axum::routing::get(vm_exec_logs_handler),
        )
        .with_state(core)
}

/// Create the router with OpenAPI documentation support
pub fn create_vm_router(core: Arc<dyn ChelseaServerCore>) -> (Router, utoipa::openapi::OpenApi) {
    let (router, api) = OpenApiRouter::new()
        .routes(routes!(vm_commit_handler))
        .routes(routes!(vm_list_all_handler))
        .routes(routes!(vm_create_handler))
        .routes(routes!(vm_update_state_handler))
        .routes(routes!(vm_from_commit_handler))
        .routes(routes!(vm_exec_handler))
        .routes(routes!(vm_exec_stream_handler))
        .routes(routes!(vm_exec_stream_attach_handler))
        .routes(routes!(vm_exec_logs_handler))
        .routes(routes!(vm_get_ssh_key_handler))
        .routes(routes!(vm_delete_handler))
        .routes(routes!(vm_status_handler))
        .routes(routes!(vm_resize_disk_handler))
        .routes(routes!(vm_sleep_handler))
        .routes(routes!(vm_wake_handler))
        .routes(routes!(vm_write_file_handler, vm_read_file_handler))
        .with_state(core)
        .split_for_parts();

    (router, api)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ApiError;
    use axum::http::Request;
    use dto_lib::chelsea_server2::{
        network::VmNetworkInfoDto, system::SystemTelemetryResponse, vm::VmResizeDiskRequest,
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    // ── Mock ChelseaServerCore ──────────────────────────────────────────

    /// Captures the arguments passed to vm_exec / vm_exec_logs so tests
    /// can verify parameter threading (vm_id, wait_boot, request fields).
    struct CapturedExec {
        vm_id: Uuid,
        request: VmExecRequest,
        wait_boot: bool,
    }

    struct CapturedExecLogs {
        vm_id: Uuid,
        query: VmExecLogQuery,
        wait_boot: bool,
    }

    struct MockCore {
        exec_result: tokio::sync::Mutex<Option<Result<VmExecResponse, ApiError>>>,
        exec_logs_result: tokio::sync::Mutex<Option<Result<VmExecLogResponse, ApiError>>>,
        /// Captured inputs for assertions.
        captured_exec: tokio::sync::Mutex<Option<CapturedExec>>,
        captured_exec_logs: tokio::sync::Mutex<Option<CapturedExecLogs>>,
    }

    impl MockCore {
        fn with_exec(resp: Result<VmExecResponse, ApiError>) -> Arc<Self> {
            Arc::new(Self {
                exec_result: tokio::sync::Mutex::new(Some(resp)),
                exec_logs_result: tokio::sync::Mutex::new(None),
                captured_exec: tokio::sync::Mutex::new(None),
                captured_exec_logs: tokio::sync::Mutex::new(None),
            })
        }

        fn with_exec_logs(resp: Result<VmExecLogResponse, ApiError>) -> Arc<Self> {
            Arc::new(Self {
                exec_result: tokio::sync::Mutex::new(None),
                exec_logs_result: tokio::sync::Mutex::new(Some(resp)),
                captured_exec: tokio::sync::Mutex::new(None),
                captured_exec_logs: tokio::sync::Mutex::new(None),
            })
        }
    }

    #[async_trait::async_trait]
    impl ChelseaServerCore for MockCore {
        async fn vm_exec(
            &self,
            vm_id: &Uuid,
            request: VmExecRequest,
            wait_boot: bool,
        ) -> Result<VmExecResponse, ApiError> {
            *self.captured_exec.lock().await = Some(CapturedExec {
                vm_id: *vm_id,
                request: request.clone(),
                wait_boot,
            });
            self.exec_result
                .lock()
                .await
                .take()
                .expect("exec_result not set")
        }

        async fn vm_exec_stream(
            &self,
            _vm_id: &Uuid,
            _request: VmExecRequest,
            _wait_boot: bool,
        ) -> Result<chelsea_lib::vsock::ExecStreamConnection, ApiError> {
            Err(ApiError::internal("not implemented in mock"))
        }

        async fn vm_exec_stream_attach(
            &self,
            _vm_id: &Uuid,
            _request: VmExecStreamAttachRequest,
            _wait_boot: bool,
        ) -> Result<chelsea_lib::vsock::ExecStreamConnection, ApiError> {
            Err(ApiError::internal("not implemented in mock"))
        }

        async fn vm_exec_logs(
            &self,
            vm_id: &Uuid,
            query: VmExecLogQuery,
            wait_boot: bool,
        ) -> Result<VmExecLogResponse, ApiError> {
            *self.captured_exec_logs.lock().await = Some(CapturedExecLogs {
                vm_id: *vm_id,
                query: query.clone(),
                wait_boot,
            });
            self.exec_logs_result
                .lock()
                .await
                .take()
                .expect("exec_logs_result not set")
        }

        // ── Stubs for non-exec methods (never called by exec routes) ──

        async fn vm_commit(
            &self,
            _: &Uuid,
            _: Uuid,
            _: bool,
            _: bool,
        ) -> Result<VmCommitResponse, ApiError> {
            unimplemented!()
        }
        async fn vm_delete(&self, _: &Uuid, _: bool) -> Result<(), ApiError> {
            unimplemented!()
        }
        async fn vm_list_all(&self) -> Result<VmListAllResponse, ApiError> {
            unimplemented!()
        }
        async fn vm_status(&self, _: &Uuid) -> Result<VmStatusResponse, ApiError> {
            unimplemented!()
        }
        async fn vm_create(&self, _: VmCreateRequest, _: bool) -> Result<Uuid, ApiError> {
            unimplemented!()
        }
        async fn vm_from_commit(&self, _: VmFromCommitRequest) -> Result<Uuid, ApiError> {
            unimplemented!()
        }
        async fn vm_update_state(
            &self,
            _: &Uuid,
            _: VmUpdateStateRequest,
            _: bool,
        ) -> Result<(), ApiError> {
            unimplemented!()
        }
        async fn vm_get_ssh_key_and_port(&self, _: &Uuid) -> Result<(String, u16), ApiError> {
            unimplemented!()
        }
        async fn vm_resize_disk(
            &self,
            _: &Uuid,
            _: VmResizeDiskRequest,
            _: bool,
        ) -> Result<(), ApiError> {
            unimplemented!()
        }
        async fn vm_notify(&self, _: &Uuid, _: VmNotifyRequest) -> Result<(), ApiError> {
            unimplemented!()
        }
        async fn get_system_telemetry(&self) -> Result<SystemTelemetryResponse, ApiError> {
            unimplemented!()
        }
        async fn vm_wireguard_target(
            &self,
            _: &Uuid,
        ) -> Result<crate::wireguard_admin::WireGuardTarget, ApiError> {
            unimplemented!()
        }
        async fn vm_network_info(&self, _: &Uuid) -> Result<VmNetworkInfoDto, ApiError> {
            unimplemented!()
        }
        async fn vm_sleep(&self, _: &Uuid, _: bool) -> Result<(), ApiError> {
            unimplemented!()
        }
        async fn vm_wake(&self, _: &Uuid, _: VmWakeRequest) -> Result<(), ApiError> {
            unimplemented!()
        }
        async fn vm_write_file(
            &self,
            _: &Uuid,
            _: VmWriteFileRequest,
            _: bool,
        ) -> Result<(), ApiError> {
            unimplemented!()
        }
        async fn vm_read_file(&self, _: &Uuid, _: &str, _: bool) -> Result<Vec<u8>, ApiError> {
            unimplemented!()
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn test_vm_id() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
    }

    fn ok_exec(exit_code: i32, stdout: &str, stderr: &str) -> Result<VmExecResponse, ApiError> {
        Ok(VmExecResponse {
            exit_code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            exec_id: None,
        })
    }

    fn exec_request(uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    // ── Exec: happy path ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_exec_success() {
        let core = MockCore::with_exec(ok_exec(0, "hello\n", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec", test_vm_id()),
                r#"{"command":["echo","hello"]}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let r: VmExecResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello\n");
        assert!(r.stderr.is_empty());
        assert!(r.exec_id.is_none());
    }

    #[tokio::test]
    async fn test_exec_nonzero_exit() {
        let core = MockCore::with_exec(ok_exec(127, "", "command not found\n"));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec", test_vm_id()),
                r#"{"command":["nonexistent"]}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let r: VmExecResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(r.exit_code, 127);
        assert_eq!(r.stderr, "command not found\n");
    }

    #[tokio::test]
    async fn test_exec_with_full_options_and_exec_id_roundtrip() {
        let exec_id = Uuid::parse_str("660e8400-e29b-41d4-a716-446655440001").unwrap();
        let core = MockCore::with_exec(Ok(VmExecResponse {
            exit_code: 0,
            stdout: "bar\n".to_string(),
            stderr: String::new(),
            exec_id: Some(exec_id),
        }));
        let app = create_exec_router(core);

        let body = serde_json::json!({
            "command": ["echo", "$FOO"],
            "exec_id": exec_id.to_string(),
            "env": {"FOO": "bar"},
            "working_dir": "/tmp",
            "timeout_secs": 10
        });

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec", test_vm_id()),
                &serde_json::to_string(&body).unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let r: VmExecResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(r.exec_id, Some(exec_id));
    }

    // ── Exec: parameter threading ───────────────────────────────────────

    #[tokio::test]
    async fn test_exec_vm_id_threaded_to_core() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core.clone());

        app.oneshot(exec_request(
            &format!("/api/vm/{}/exec", test_vm_id()),
            r#"{"command":["ls"]}"#,
        ))
        .await
        .unwrap();

        let captured = core.captured_exec.lock().await;
        let c = captured.as_ref().unwrap();
        assert_eq!(c.vm_id, test_vm_id());
        assert_eq!(c.request.command, vec!["ls"]);
    }

    #[tokio::test]
    async fn test_exec_skip_wait_boot_default_is_false() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core.clone());

        // No query param → skip_wait_boot defaults to None → wait_boot = true
        app.oneshot(exec_request(
            &format!("/api/vm/{}/exec", test_vm_id()),
            r#"{"command":["ls"]}"#,
        ))
        .await
        .unwrap();

        let captured = core.captured_exec.lock().await;
        assert!(captured.as_ref().unwrap().wait_boot);
    }

    #[tokio::test]
    async fn test_exec_skip_wait_boot_true_inverts() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core.clone());

        app.oneshot(exec_request(
            &format!("/api/vm/{}/exec?skip_wait_boot=true", test_vm_id()),
            r#"{"command":["ls"]}"#,
        ))
        .await
        .unwrap();

        let captured = core.captured_exec.lock().await;
        // skip_wait_boot=true → wait_boot should be false
        assert!(!captured.as_ref().unwrap().wait_boot);
    }

    // ── Exec: error cases ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_exec_internal_error_returns_500() {
        let core = MockCore::with_exec(Err(ApiError::internal("vsock connection refused")));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec", test_vm_id()),
                r#"{"command":["ls"]}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("vsock connection refused"));
    }

    #[tokio::test]
    async fn test_exec_missing_command_field_returns_422() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec", test_vm_id()),
                r#"{}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_exec_empty_command_array_accepted() {
        // An empty command array is syntactically valid JSON — validation
        // is the agent's responsibility, not the HTTP layer's.
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core.clone());

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec", test_vm_id()),
                r#"{"command":[]}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let captured = core.captured_exec.lock().await;
        assert!(captured.as_ref().unwrap().request.command.is_empty());
    }

    #[tokio::test]
    async fn test_exec_invalid_json_returns_400() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec", test_vm_id()),
                r#"not json at all"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_exec_invalid_vm_id_returns_400() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                "/api/vm/not-a-uuid/exec",
                r#"{"command":["ls"]}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_exec_missing_content_type_returns_415() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/vm/{}/exec", test_vm_id()))
                    // no content-type header
                    .body(Body::from(r#"{"command":["ls"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn test_exec_empty_body_returns_400() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/vm/{}/exec", test_vm_id()))
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_exec_wrong_http_method_returns_405() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/vm/{}/exec", test_vm_id()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    // ── Exec logs: happy path ───────────────────────────────────────────

    #[tokio::test]
    async fn test_exec_logs_success() {
        let core = MockCore::with_exec_logs(Ok(VmExecLogResponse {
            entries: vec![VmExecLogEntry {
                exec_id: None,
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                stream: VmExecLogStream::Stdout,
                data_b64: "aGVsbG8=".to_string(),
            }],
            next_offset: 42,
            eof: true,
        }));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/vm/{}/exec/logs", test_vm_id()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let logs: VmExecLogResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(logs.entries.len(), 1);
        assert_eq!(logs.entries[0].stream, VmExecLogStream::Stdout);
        assert_eq!(logs.next_offset, 42);
        assert!(logs.eof);
    }

    #[tokio::test]
    async fn test_exec_logs_empty_entries() {
        let core = MockCore::with_exec_logs(Ok(VmExecLogResponse {
            entries: vec![],
            next_offset: 0,
            eof: true,
        }));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/vm/{}/exec/logs", test_vm_id()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let logs: VmExecLogResponse = serde_json::from_slice(&body).unwrap();
        assert!(logs.entries.is_empty());
    }

    // ── Exec logs: parameter threading ──────────────────────────────────

    #[tokio::test]
    async fn test_exec_logs_query_params_threaded() {
        let core = MockCore::with_exec_logs(Ok(VmExecLogResponse {
            entries: vec![],
            next_offset: 0,
            eof: true,
        }));
        let app = create_exec_router(core.clone());

        app.oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/api/vm/{}/exec/logs?offset=50&max_entries=10&stream=stderr&skip_wait_boot=true",
                    test_vm_id()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        let captured = core.captured_exec_logs.lock().await;
        let c = captured.as_ref().unwrap();
        assert_eq!(c.vm_id, test_vm_id());
        assert_eq!(c.query.offset, Some(50));
        assert_eq!(c.query.max_entries, Some(10));
        assert_eq!(c.query.stream, Some(VmExecLogStream::Stderr));
        // skip_wait_boot=true → wait_boot=false
        assert!(!c.wait_boot);
    }

    #[tokio::test]
    async fn test_exec_logs_no_query_params_uses_defaults() {
        let core = MockCore::with_exec_logs(Ok(VmExecLogResponse {
            entries: vec![],
            next_offset: 0,
            eof: true,
        }));
        let app = create_exec_router(core.clone());

        app.oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/vm/{}/exec/logs", test_vm_id()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        let captured = core.captured_exec_logs.lock().await;
        let c = captured.as_ref().unwrap();
        assert!(c.query.offset.is_none());
        assert!(c.query.max_entries.is_none());
        assert!(c.query.stream.is_none());
        assert!(c.wait_boot); // default: wait
    }

    // ── Exec logs: error cases ──────────────────────────────────────────

    #[tokio::test]
    async fn test_exec_logs_internal_error() {
        let core = MockCore::with_exec_logs(Err(ApiError::internal("agent unreachable")));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/vm/{}/exec/logs", test_vm_id()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_exec_logs_invalid_vm_id() {
        let core = MockCore::with_exec_logs(Ok(VmExecLogResponse {
            entries: vec![],
            next_offset: 0,
            eof: true,
        }));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/vm/not-a-uuid/exec/logs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_exec_logs_post_method_not_allowed() {
        let core = MockCore::with_exec_logs(Ok(VmExecLogResponse {
            entries: vec![],
            next_offset: 0,
            eof: true,
        }));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/vm/{}/exec/logs", test_vm_id()))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    // ── Exec stream stubs (no real vsock in unit tests) ─────────────────

    #[tokio::test]
    async fn test_exec_stream_returns_500_from_unimplemented_mock() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec/stream", test_vm_id()),
                r#"{"command":["ls"]}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_exec_stream_attach_returns_500_from_unimplemented_mock() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let body = serde_json::json!({ "exec_id": test_vm_id().to_string() });
        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec/stream/attach", test_vm_id()),
                &serde_json::to_string(&body).unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_exec_stream_attach_missing_exec_id_returns_422() {
        let core = MockCore::with_exec(ok_exec(0, "", ""));
        let app = create_exec_router(core);

        let resp = app
            .oneshot(exec_request(
                &format!("/api/vm/{}/exec/stream/attach", test_vm_id()),
                r#"{}"#,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
