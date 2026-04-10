use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, Query},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use dto_lib::{ErrorResponse, chelsea_server2::vm::VmCreateVmConfig};
use dto_lib::{
    chelsea_server2::vm::{
        VmCommitQuery, VmCommitRequest, VmCommitResponse, VmDeleteQuery, VmExecLogQuery,
        VmExecLogResponse, VmExecRequest, VmExecResponse, VmExecStreamAttachRequest,
        VmReadFileResponse, VmResizeDiskQuery, VmResizeDiskRequest, VmSshKeyResponse,
        VmUpdateStateEnum, VmUpdateStateQuery, VmUpdateStateRequest, VmWriteFileRequest,
    },
    orchestrator::vm::{BranchQuery, BranchVmQuery, VmMetadataResponse},
};

use crate::{
    action::{
        self, AuthzError, Branch, BranchVMError, CommitVM, DeleteVM, ExecVM, FromCommitVM,
        GetCommit, GetCommitError, GetVMExecLogs, GetVMMetadata, GetVMSshKey, GetVMStatus,
        GetVMStatusError, LabelVM, ListAllVMs, ListParentCommits, NewRootVM, ReadFileVM,
        ResizeVMDisk, UpdateVMState, VM, WriteFileVM, check_vm_access,
    },
    db::{ChelseaNodeRepository, VmCommitEntity},
    inbound::{InboundState, OperationId, extractors::AuthApiKey},
};

/// Request body for POST /api/v1/vm/from_commit
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum FromCommitVmRequest {
    /// The commit ID to restore from
    CommitId(Uuid),
    /// The tag name to restore from (legacy org-scoped tag)
    TagName(String),
    /// A repository reference in "repo_name:tag_name" format
    #[serde(rename = "ref")]
    Ref(String),
}

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct NewVmsResponse {
    pub vms: Vec<NewVmResponse>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_error",
        skip_deserializing
    )]
    #[schema(value_type = Option<String>)]
    pub error: Option<BranchVMError>,
}

fn serialize_error<S>(error: &Option<BranchVMError>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match error {
        Some(e) => serializer.serialize_str(&e.to_string()),
        None => serializer.serialize_none(),
    }
}

impl NewVmsResponse {
    pub fn with_error(vms: Vec<NewVmResponse>, error: BranchVMError) -> Self {
        Self {
            vms,
            error: Some(error),
        }
    }

    pub fn without_error(vms: Vec<NewVmResponse>) -> Self {
        Self { vms, error: None }
    }

    fn into_http_response(self) -> axum::response::Response {
        // Determine status code based on error type if present
        let status_code = match &self.error {
            Some(err) => match err {
                BranchVMError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BranchVMError::DbChelsea(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BranchVMError::Http(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BranchVMError::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
                BranchVMError::Forbidden => StatusCode::FORBIDDEN,
                BranchVMError::ParentVMNotFound => StatusCode::NOT_FOUND,
                BranchVMError::ChooseNodeError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BranchVMError::CommitNotFound => StatusCode::NOT_FOUND,
                BranchVMError::CommitVm(_) => StatusCode::INTERNAL_SERVER_ERROR,
                BranchVMError::TagNotFound => StatusCode::NOT_FOUND,
                BranchVMError::ResourceLimitExceeded(_) => StatusCode::FORBIDDEN,
            },
            None => StatusCode::CREATED, // Success case
        };

        (status_code, Json(self)).into_response()
    }
}

/// Response body for new VM requests (new_root, from_commit, branch)
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone)]
pub struct NewVmResponse {
    pub vm_id: String,
}

/// Response body for DELETE /api/vm/{vm_id}
#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct VmDeleteResponse {
    pub vm_id: String,
}

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct NewRootRequest {
    pub vm_config: VmCreateVmConfig,
}

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct LabelVmRequest {
    pub labels: Option<HashMap<String, String>>,
}

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
    post,
    path = "/new_root",
    request_body = NewRootRequest,
    params(
        ("wait_boot" = Option<bool>, Query, description = "If true, wait for the newly-created VM to finish booting before returning. Default: false.")
    ),
    responses(
        (status = 201, description = "VM created successfully", body = NewVmResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn create_new_root_vm(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Query(query): Query<dto_lib::chelsea_server2::vm::VmCreateQuery>,
    Json(req): Json<NewRootRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            NewRootVM::new(req.vm_config, key, query.wait_boot.unwrap_or(false))
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/{vm_id}/label",
    params(),
    request_body = LabelVmRequest,
    responses(
        (status = 200, description = "Labels set successfully"),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn label_vm(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Json(req): Json<LabelVmRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            LabelVM::new(vm_id, req.labels, key)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
pub struct QueryCommit {
    count: Option<u8>,
}

#[utoipa::path(
    post,
    path = "/branch/by_commit/{commit_id}",
    params(
        ("commit_id" = String, Path, description = "The commit id to branch off"),
        ("count" = Option<u8>, Query, description = "Number of VMs to branch (optional; default 1)")
    ),
    responses(
        (status = 201, description = "Branch VM(s) created successfully", body = NewVmsResponse),
        (status = 400, description = "Invalid VM ID", body = NewVmsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = NewVmsResponse),
        (status = 404, description = "VM not found", body = NewVmsResponse),
        (status = 500, description = "Internal server error", body = NewVmsResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn branch_by_commit(
    Extension(state): Extension<InboundState>,
    operation_id: OperationId,
    AuthApiKey(key_entity): AuthApiKey,
    Path(commit_id): Path<Uuid>,
    Query(query): Query<QueryCommit>,
) -> impl IntoResponse {
    let commit = match GetCommit::by_id(commit_id, key_entity.clone())
        .call(&state.db)
        .await
    {
        Ok(commit) => commit,
        Err(err) => {
            return match err {
                GetCommitError::Db(_) => ErrorResponse::internal_server_error(None),
                GetCommitError::NotFound => {
                    ErrorResponse::not_found(Some("commit not found".into()))
                }
                GetCommitError::Http(_) => ErrorResponse::internal_server_error(None),
                GetCommitError::InternalServerError => ErrorResponse::internal_server_error(None),
                GetCommitError::Forbidden => ErrorResponse::forbidden(None),
            }
            .into_response();
        }
    };

    match action_http!(
        action::call(
            Branch::by_commit(key_entity, commit, None, query.count)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_http_response(),
    }
}

#[utoipa::path(
    post,
    path = "/branch/by_tag/{tag_name}",
    params(
        ("tag_name" = String, Path, description = "The tag name to branch off"),
        ("count" = Option<u8>, Query, description = "Number of VMs to branch (optional; default 1)")
    ),
    responses(
        (status = 201, description = "Branch VM(s) created successfully", body = NewVmsResponse),
        (status = 400, description = "Invalid request", body = NewVmsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = NewVmsResponse),
        (status = 404, description = "Tag not found", body = NewVmsResponse),
        (status = 500, description = "Internal server error", body = NewVmsResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn branch_by_tag(
    AuthApiKey(key_entity): AuthApiKey,
    Path(tag_name): Path<String>,
    Query(query): Query<QueryCommit>,
) -> impl IntoResponse {
    match action_http!(action::call(Branch::by_tag(key_entity, tag_name, None, query.count)).await)
    {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_http_response(),
    }
}

#[utoipa::path(
    post,
    path = "/branch/by_ref/{repo_name}/{tag_name}",
    params(
        ("repo_name" = String, Path, description = "The repository name"),
        ("tag_name" = String, Path, description = "The tag name within the repository"),
        ("count" = Option<u8>, Query, description = "Number of VMs to branch (optional; default 1)")
    ),
    responses(
        (status = 201, description = "Branch VM(s) created successfully", body = NewVmsResponse),
        (status = 400, description = "Invalid request", body = NewVmsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = NewVmsResponse),
        (status = 404, description = "Repository or tag not found", body = NewVmsResponse),
        (status = 500, description = "Internal server error", body = NewVmsResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn branch_by_ref(
    AuthApiKey(key_entity): AuthApiKey,
    Path((repo_name, tag_name)): Path<(String, String)>,
    Query(query): Query<QueryCommit>,
) -> impl IntoResponse {
    match action_http!(
        action::call(Branch::by_ref(
            key_entity,
            repo_name,
            tag_name,
            None,
            query.count
        ))
        .await
    ) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_http_response(),
    }
}

#[utoipa::path(
    post,
    path = "/branch/by_vm/{vm_id}",
    params(
        ("vm_id" = String, Path, description = "VM to commit and then branch off of"),
        ("keep_paused" = Option<bool>, Query, description = "If true, keep VM paused after commit"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, immediately return an error if VM is booting instead of waiting"),
        ("count" = Option<u8>, Query, description = "Number of VMs to branch (optional; default 1)")
    ),
    responses(
        (status = 201, description = "Branch VM(s) created successfully", body = NewVmsResponse),
        (status = 400, description = "Invalid VM ID", body = NewVmsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = NewVmsResponse),
        (status = 404, description = "VM not found", body = NewVmsResponse),
        (status = 500, description = "Internal server error", body = NewVmsResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn branch_by_vm(
    operation_id: OperationId,
    AuthApiKey(key_entity): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<BranchVmQuery>,
) -> impl IntoResponse {
    let commit_id = Uuid::new_v4();
    let keep_paused = query.keep_paused.unwrap_or(false);
    let skip_wait_boot = query.skip_wait_boot.unwrap_or(false);

    match action_http!(
        action::call(
            Branch::by_vm(
                key_entity,
                vm_id,
                commit_id,
                None,
                keep_paused,
                skip_wait_boot,
                query.count
            )
            .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_http_response(),
    }
}

#[utoipa::path(
    post,
    path = "/{vm_or_commit_id}/branch",
    params(
        ("vm_or_commit_id" = String, Path, description = "Parent VM or commit ID"),
        ("keep_paused" = Option<bool>, Query, description = "If true, keep VM paused after commit. Only applicable when branching a VM ID."),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, immediately return an error if VM is booting instead of waiting. Only applicable when branching a VM ID."),
        ("count" = Option<u8>, Query, description = "Number of VMs to branch (optional; default 1)")
    ),
    responses(
        (status = 201, description = "Branch VM created successfully", body = NewVmsResponse),
        (status = 400, description = "Invalid VM ID", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn branch_vm(
    _state: Extension<InboundState>,
    operation_id: OperationId,
    AuthApiKey(key_entity): AuthApiKey,
    Path(vm_or_commit_id): Path<Uuid>,
    Query(query): Query<BranchQuery>,
) -> impl IntoResponse {
    let result = action_http!(
        action::call(
            GetVMStatus::by_id(vm_or_commit_id, key_entity.clone())
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    );

    match result {
        // user provided valid vm id. use branch by_vm
        Ok(_) => branch_by_vm(
            operation_id,
            AuthApiKey(key_entity),
            Path(vm_or_commit_id),
            Query(BranchVmQuery::from(query)),
        )
        .await
        .into_response(),
        Err(err) => match err {
            // VM doesn't exist. customer prob wants commit.
            GetVMStatusError::NotFound => branch_by_commit(
                _state,
                operation_id,
                AuthApiKey(key_entity),
                Path(vm_or_commit_id),
                Query(QueryCommit { count: query.count }),
            )
            .await
            .into_response(),
            _ => err.into_response(),
        },
    }
}

#[utoipa::path(
    post,
    path = "/{vm_id}/commit",
    params(
        ("vm_id" = String, Path, description = "VM ID to commit"),
        ("keep_paused" = Option<bool>, Query, description = "If true, keep VM paused after commit"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false")
    ),
    request_body(content = VmCommitRequest, description = "Optional commit metadata", content_type = "application/json"),
    responses(
        (status = 201, description = "VM committed successfully", body = VmCommitResponse),
        (status = 400, description = "Invalid VM ID", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn commit_vm(
    operation_id: OperationId,
    AuthApiKey(key_entity): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    query: Query<VmCommitQuery>,
    body: Option<Json<VmCommitRequest>>,
) -> impl IntoResponse {
    let body = body.map(|b| b.0).unwrap_or_default();
    let commit_id = body.commit_id.unwrap_or_else(Uuid::new_v4);
    let name = body.name.filter(|s| !s.trim().is_empty());
    let description = body.description.filter(|s| !s.trim().is_empty());

    let mut action = CommitVM::new(
        vm_id,
        commit_id,
        key_entity,
        query.keep_paused.unwrap_or(false),
        query.skip_wait_boot.unwrap_or(false),
    )
    .with_request_id(Some(operation_id.as_str().to_string()));

    if let Some(name) = name {
        action = action.with_name(name);
    }
    action = action.with_description(description);

    match action_http!(action::call(action).await) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/from_commit",
    request_body = FromCommitVmRequest,
    responses(
        (status = 201, description = "VM restored from commit successfully", body = NewVmResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Cluster not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn restore_from_commit(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Json(req): Json<FromCommitVmRequest>,
) -> impl IntoResponse {
    // Build commit identifier from request
    let commit_identifier = match req {
        FromCommitVmRequest::CommitId(commit_id) => {
            crate::action::CommitIdentifier::CommitId(commit_id)
        }
        FromCommitVmRequest::TagName(tag_name) => {
            crate::action::CommitIdentifier::TagName(tag_name)
        }
        FromCommitVmRequest::Ref(reference) => {
            // Parse "repo_name:tag_name" format
            match reference.split_once(':') {
                Some((repo_name, tag_name)) if !repo_name.is_empty() && !tag_name.is_empty() => {
                    crate::action::CommitIdentifier::Ref {
                        repo_name: repo_name.to_string(),
                        tag_name: tag_name.to_string(),
                    }
                }
                _ => {
                    return ErrorResponse::bad_request(Some(
                        "Invalid ref format. Expected 'repo_name:tag_name'".to_string(),
                    ))
                    .into_response();
                }
            }
        }
    };

    match action_http!(
        action::call(
            FromCommitVM::new(commit_identifier, key)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/{vm_id}/state",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, error immediately if the VM is not finished booting. Defaults to false")
    ),
    request_body = VmUpdateStateRequest,
    responses(
        (status = 200, description = "VM state updated successfully"),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn update_vm_state(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmUpdateStateQuery>,
    Json(req): Json<VmUpdateStateRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            UpdateVMState::new(vm_id, req.state, key, query.skip_wait_boot.unwrap_or(false))
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/{vm_id}/disk",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID whose disk to resize"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false")
    ),
    request_body = VmResizeDiskRequest,
    responses(
        (status = 200, description = "VM disk resized successfully"),
        (status = 400, description = "Invalid request (e.g. new size not larger than current)", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn resize_vm_disk(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmResizeDiskQuery>,
    Json(req): Json<VmResizeDiskRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            ResizeVMDisk::new(
                vm_id,
                req.fs_size_mib,
                key,
                query.skip_wait_boot.unwrap_or(false)
            )
            .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/{vm_id}",
    params(
        ("vm_id" = String, Path, description = "VM ID to delete"),
        ("skip_wait_boot" = Option<bool>, Query, description = "If true, return an error immediately if the VM is still booting. Default: false")
    ),
    responses(
        (status = 200, description = "VM deleted successfully", body = VmDeleteResponse),
        (status = 400, description = "Invalid VM ID", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn delete_vm(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmDeleteQuery>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            DeleteVM::new(vm_id, key, query.skip_wait_boot.unwrap_or(false))
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(deleted_id) => {
            let response = VmDeleteResponse {
                vm_id: deleted_id.to_string(),
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "",
    responses(
        (status = 200, description = "List all VMs accessible to the API key", body = Vec<VM>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vms"
)]
pub async fn list_vms(operation_id: OperationId, AuthApiKey(key): AuthApiKey) -> impl IntoResponse {
    match action_http!(
        action::call(ListAllVMs::new(key).with_request_id(Some(operation_id.as_str().to_string())))
            .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── File Transfer ───────────────────────────────────────────────────

/// Query params for GET /{vm_id}/files
#[derive(Deserialize, ToSchema)]
pub struct ReadFileQuery {
    /// Absolute path of the file to read on the VM.
    pub path: String,
}

#[utoipa::path(
    put,
    path = "/{vm_id}/files",
    params(("vm_id" = Uuid, Path, description = "VM ID")),
    request_body = VmWriteFileRequest,
    responses(
        (status = 200, description = "File written successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn write_file_vm(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Json(request): Json<VmWriteFileRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            WriteFileVM::new(vm_id, key, request)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{vm_id}/files",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID"),
        ("path" = String, Query, description = "Absolute path of the file to read"),
    ),
    responses(
        (status = 200, description = "File contents", body = VmReadFileResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn read_file_vm(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<ReadFileQuery>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            ReadFileVM::new(vm_id, key, query.path)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── Exec ────────────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/{vm_id}/exec",
    params(("vm_id" = Uuid, Path, description = "VM ID")),
    request_body = VmExecRequest,
    responses(
        (status = 200, description = "Command executed successfully", body = VmExecResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn exec_vm(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Json(request): Json<VmExecRequest>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            ExecVM::new(vm_id, key, request)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── Exec Stream ─────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/{vm_id}/exec/stream",
    params(("vm_id" = Uuid, Path, description = "VM ID")),
    request_body = VmExecRequest,
    responses(
        (status = 200, description = "NDJSON stream of stdout/stderr chunks"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn exec_vm_stream(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Json(request): Json<VmExecRequest>,
) -> impl IntoResponse {
    let ctx = action::context();
    let request_id = operation_id.as_str().to_string();

    let vm = match check_vm_access(&ctx.db, &key, vm_id).await {
        Ok(vm) => vm,
        Err(AuthzError::VmNotFound) => {
            return ErrorResponse::not_found(Some("not found".to_string())).into_response();
        }
        Err(AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound) => {
            return ErrorResponse::forbidden(Some("forbidden".to_string())).into_response();
        }
        Err(AuthzError::Db(e)) => {
            return ErrorResponse::internal_server_error(Some(e.to_string())).into_response();
        }
    };

    let node_id = match vm.node_id {
        Some(node_id) => node_id,
        None => {
            return ErrorResponse::internal_server_error(Some("node not found".to_string()))
                .into_response();
        }
    };

    let node = match ctx.db.node().get_by_id(&node_id).await {
        Ok(Some(node)) => node,
        Ok(None) => {
            return ErrorResponse::internal_server_error(Some("node not found".to_string()))
                .into_response();
        }
        Err(e) => return ErrorResponse::internal_server_error(Some(e.to_string())).into_response(),
    };

    let upstream = match ctx
        .proto()
        .vm_exec_stream(&node, vm.id(), request, Some(&request_id))
        .await
    {
        Ok(resp) => resp,
        Err(e) => return ErrorResponse::internal_server_error(Some(e.to_string())).into_response(),
    };

    let body = Body::from_stream(upstream.bytes_stream());
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    response.into_response()
}

// ── Exec Stream Attach ──────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/{vm_id}/exec/stream/attach",
    params(("vm_id" = Uuid, Path, description = "VM ID")),
    request_body = VmExecStreamAttachRequest,
    responses(
        (status = 200, description = "NDJSON stream replaying from cursor"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn exec_vm_stream_attach(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Json(request): Json<VmExecStreamAttachRequest>,
) -> impl IntoResponse {
    let ctx = action::context();
    let request_id = operation_id.as_str().to_string();

    let vm = match check_vm_access(&ctx.db, &key, vm_id).await {
        Ok(vm) => vm,
        Err(AuthzError::VmNotFound) => {
            return ErrorResponse::not_found(Some("not found".to_string())).into_response();
        }
        Err(AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound) => {
            return ErrorResponse::forbidden(Some("forbidden".to_string())).into_response();
        }
        Err(AuthzError::Db(e)) => {
            return ErrorResponse::internal_server_error(Some(e.to_string())).into_response();
        }
    };

    let node_id = match vm.node_id {
        Some(node_id) => node_id,
        None => {
            return ErrorResponse::internal_server_error(Some("node not found".to_string()))
                .into_response();
        }
    };

    let node = match ctx.db.node().get_by_id(&node_id).await {
        Ok(Some(node)) => node,
        Ok(None) => {
            return ErrorResponse::internal_server_error(Some("node not found".to_string()))
                .into_response();
        }
        Err(e) => return ErrorResponse::internal_server_error(Some(e.to_string())).into_response(),
    };

    let upstream = match ctx
        .proto()
        .vm_exec_stream_attach(&node, vm.id(), request, Some(&request_id))
        .await
    {
        Ok(resp) => resp,
        Err(e) => return ErrorResponse::internal_server_error(Some(e.to_string())).into_response(),
    };

    let body = Body::from_stream(upstream.bytes_stream());
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    response.into_response()
}

// ── Logs ────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/{vm_id}/logs",
    params(
        ("vm_id" = Uuid, Path, description = "VM ID"),
        ("offset" = Option<u64>, Query, description = "Byte offset into the log file (default: 0)"),
        ("max_entries" = Option<u32>, Query, description = "Maximum number of log entries to return"),
        ("stream" = Option<String>, Query, description = "Filter by 'stdout' or 'stderr'"),
    ),
    responses(
        (status = 200, description = "Exec logs retrieved", body = VmExecLogResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn vm_logs(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
    Query(query): Query<VmExecLogQuery>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            GetVMExecLogs::new(vm_id, key, query)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

// ── Status ──────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/{vm_id}/status",
    params(("vm_id" = Uuid, Path, description = "VM ID")),
    responses(
        (status = 200, description = "Get status of a specific VM", body = VM),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn vm_status(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            GetVMStatus::by_id(vm_id, key).with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{vm_id}/ssh_key",
    params(("vm_id" = Uuid, Path, description = "Node ID")),
    responses(
        (status = 200, description = "List all VMs on node", body = VmSshKeyResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn ssh_key(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            GetVMSshKey::by_id(vm_id, key).with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/commits/{commit_id}/parents",
    params(
        ("commit_id" = Uuid, Path, description = "Commit ID to start from")
    ),
    responses(
        (status = 200, description = "List of commits from the specified commit to the root", body = Vec<VmCommitEntity>),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Commit not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "commits"
)]
pub async fn list_parent_commits(
    Extension(state): Extension<InboundState>,
    AuthApiKey(key): AuthApiKey,
    Path(commit_id): Path<Uuid>,
) -> impl IntoResponse {
    match ListParentCommits::new(commit_id, key).call(&state.db).await {
        Ok(commits) => (StatusCode::OK, Json(commits)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/{vm_id}/metadata",
    params(("vm_id" = Uuid, Path, description = "VM ID")),
    responses(
        (status = 200, description = "VM metadata", body = VmMetadataResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "VM not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    ),
    security(("bearer_auth" = [])),
    tag = "vm"
)]
pub async fn get_vm_metadata(
    operation_id: OperationId,
    AuthApiKey(key): AuthApiKey,
    Path(vm_id): Path<Uuid>,
) -> impl IntoResponse {
    match action_http!(
        action::call(
            GetVMMetadata::by_id(vm_id, key)
                .with_request_id(Some(operation_id.as_str().to_string()))
        )
        .await
    ) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        create_new_root_vm,
        branch_by_vm,
        branch_by_commit,
        branch_by_tag,
        branch_by_ref,
        branch_vm,
        commit_vm,
        restore_from_commit,
        update_vm_state,
        resize_vm_disk,
        delete_vm,
        ssh_key,
        vm_status,
        list_parent_commits,
        get_vm_metadata,
        exec_vm,
        exec_vm_stream,
        exec_vm_stream_attach,
        vm_logs,
        write_file_vm,
        read_file_vm,
    ),
    components(schemas(
        NewRootRequest,
        NewVmResponse,
        NewVmsResponse,
        FromCommitVmRequest,
        VmDeleteResponse,
        VmCommitRequest,
        VmCommitResponse,
        VmUpdateStateRequest,
        VmUpdateStateEnum,
        VmResizeDiskRequest,
        VmSshKeyResponse,
        VmMetadataResponse,
        VmExecRequest,
        VmExecResponse,
        VmExecStreamAttachRequest,
        VmExecLogQuery,
        VmExecLogResponse,
        VmWriteFileRequest,
        VmReadFileResponse,
        ErrorResponse,
        VM,
        VmCommitEntity
    ))
)]
pub struct VmControlApiDoc;

pub fn vm_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(VmControlApiDoc::openapi())
        .routes(routes!(create_new_root_vm))
        .routes(routes!(label_vm))
        .routes(routes!(restore_from_commit))
        .routes(routes!(branch_by_vm))
        .routes(routes!(branch_by_commit))
        .routes(routes!(branch_by_tag))
        .routes(routes!(branch_by_ref))
        .routes(routes!(branch_vm))
        .routes(routes!(commit_vm))
        .routes(routes!(update_vm_state))
        .routes(routes!(resize_vm_disk))
        .routes(routes!(ssh_key))
        .routes(routes!(delete_vm))
        .routes(routes!(exec_vm))
        .routes(routes!(exec_vm_stream))
        .routes(routes!(exec_vm_stream_attach))
        .routes(routes!(vm_logs))
        .routes(routes!(vm_status))
        .routes(routes!(get_vm_metadata))
        .routes(routes!(write_file_vm, read_file_vm))
        .routes(routes!(list_vms))
        .routes(routes!(list_parent_commits))
}

#[derive(OpenApi)]
#[openapi(paths(list_vms,), components(schemas(VM, ErrorResponse,)))]
pub struct VmsControlApiDoc;

pub fn vms_routes() -> OpenApiRouter {
    OpenApiRouter::with_openapi(VmsControlApiDoc::openapi()).routes(routes!(list_vms))
}
