use std::collections::HashMap;

mod setup_script;

use chrono::Utc;
use orch_wg::{gen_private_key, gen_public_key};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;
use vers_config::VersConfig;

use crate::action::{self, Action, ActionError, ChooseNode, RechooseNodeError, VmRequirements};
use crate::db::{
    ApiKeyEntity, CheckAndInsertError, ChelseaNodeRepository, DBError, DeployRepository,
    EnvVarsRepository, OrgsRepository, VMCommitsRepository, VMsRepository, VmInsertParams,
};
use crate::outbound::{github, node_proto::HttpError};
use dto_lib::chelsea_server2::vm::{
    VmCreateRequest, VmCreateVmConfig, VmExecRequest, VmWireGuardConfig,
};

pub use setup_script::build_setup_script;

/// Maximum retries for WireGuard port collisions on a single node.
const MAX_WG_PORT_RETRIES: u8 = 10;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Settings for the deploy build pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct DeploySettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
}

/// Request body for `POST /api/v1/deploy`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployRequest {
    /// GitHub repository in `owner/repo` format.
    pub repo: String,
    /// Optional project name (defaults to repo name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Git branch to clone (defaults to repo default branch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Build/run settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<DeploySettings>,
}

/// Response body for `POST /api/v1/deploy`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeployResponse {
    pub project_id: Uuid,
    pub vm_id: Uuid,
    pub status: String,
}

// ---------------------------------------------------------------------------
// Action
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeployFromGitHub {
    pub request: DeployRequest,
    pub api_key: ApiKeyEntity,
    pub request_id: Option<String>,
}

impl DeployFromGitHub {
    pub fn new(request: DeployRequest, api_key: ApiKeyEntity) -> Self {
        Self {
            request,
            api_key,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum DeployError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("forbidden")]
    Forbidden,
    #[error("GitHub App not configured")]
    GitHubNotConfigured,
    #[error("GitHub App not installed for this organization")]
    GitHubNotInstalled,
    #[error("repository not found: {0}")]
    RepoNotFound(String),
    #[error("project name already exists: {0}")]
    ProjectNameConflict(String),
    #[error("failed to get clone URL: {0}")]
    CloneUrlFailed(String),
    #[error("no available nodes")]
    ChooseNodeError(#[from] RechooseNodeError),
    #[error("VM provisioning failed: {0}")]
    VmProvisioningFailed(#[from] HttpError),
    #[error("internal server error")]
    InternalServerError,
    #[error("invalid request: {0}")]
    BadRequest(String),

    #[error("{0}")]
    ResourceLimitExceeded(#[from] super::vms::ResourceLimitError),
}

impl From<CheckAndInsertError> for DeployError {
    fn from(e: CheckAndInsertError) -> Self {
        match e {
            CheckAndInsertError::Db(db) => DeployError::Db(db),
            CheckAndInsertError::ResourceLimit(rl) => DeployError::ResourceLimitExceeded(rl),
            CheckAndInsertError::NotUniqueNodeIdWgPortCombination => {
                DeployError::InternalServerError
            }
        }
    }
}

impl Action for DeployFromGitHub {
    type Response = DeployResponse;
    type Error = DeployError;
    const ACTION_ID: &'static str = "deploy.from_github";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // ------------------------------------------------------------------
        // 1. Validate GitHub App is configured
        // ------------------------------------------------------------------
        if !github::is_configured() {
            return Err(DeployError::GitHubNotConfigured);
        }

        // ------------------------------------------------------------------
        // 2. Validate org access
        // ------------------------------------------------------------------
        let Some(_org) = ctx.db.orgs().get_by_id(self.api_key.org_id()).await? else {
            return Err(DeployError::Forbidden);
        };
        let org_id = self.api_key.org_id();

        // ------------------------------------------------------------------
        // 3. Validate repo format
        // ------------------------------------------------------------------
        let repo = &self.request.repo;
        if !repo.contains('/') || repo.split('/').count() != 2 {
            return Err(DeployError::BadRequest(
                "repo must be in owner/repo format".into(),
            ));
        }

        // ------------------------------------------------------------------
        // 4. Look up the GitHub repo + installation
        // ------------------------------------------------------------------
        let installation = ctx
            .db
            .deploy()
            .find_installation_by_org(org_id)
            .await?
            .ok_or(DeployError::GitHubNotInstalled)?;

        let gh_repo = ctx
            .db
            .deploy()
            .find_repo_by_full_name_and_org(repo, org_id)
            .await?
            .ok_or_else(|| DeployError::RepoNotFound(repo.clone()))?;

        // ------------------------------------------------------------------
        // 5. Derive project name
        // ------------------------------------------------------------------
        let project_name = self
            .request
            .name
            .clone()
            .unwrap_or_else(|| gh_repo.github_repo_name.clone());

        if project_name.len() < 3 {
            return Err(DeployError::BadRequest(
                "project name must be at least 3 characters".into(),
            ));
        }

        // ------------------------------------------------------------------
        // 6. Get clone URL
        // ------------------------------------------------------------------
        let clone_url =
            github::get_clone_url(installation.installation_id, &gh_repo.github_repo_full_name)
                .await
                .map_err(|e| DeployError::CloneUrlFailed(e.to_string()))?;

        let branch = self
            .request
            .branch
            .clone()
            .unwrap_or(gh_repo.github_repo_default_branch.clone());

        // ------------------------------------------------------------------
        // 7. Create VM (same logic as NewRootVM)
        // ------------------------------------------------------------------
        let mut labels = HashMap::new();
        labels.insert(String::from("project"), project_name.clone());
        let vm_config = VmCreateVmConfig {
            image_name: Some("default".into()),
            kernel_name: Some("default.bin".into()),
            mem_size_mib: Some(256),
            vcpu_count: Some(1),
            fs_size_mib: Some(1024),
            labels: Some(labels.clone()),
        };

        let requirements =
            VmRequirements::from_optional(vm_config.vcpu_count, vm_config.mem_size_mib);

        // Resource limits are enforced atomically inside check_limits_and_insert_vm.

        let mut candidates =
            match action::call(ChooseNode::new().with_requirements(requirements)).await {
                Ok(c) => c,
                Err(ActionError::Error(err)) => return Err(err.into()),
                Err(_) => return Err(DeployError::InternalServerError),
            };

        let vm_id = Uuid::new_v4();
        let mut vm_created = false;
        let mut last_provision_err: Option<HttpError> = None;

        while let Some(reservation) = candidates.next_node() {
            let Some(node) = ctx.db.node().get_by_id(&reservation.node_id()).await? else {
                continue;
            };
            let node_id = *node.id();

            let vm_wg_private_key = gen_private_key();
            let vm_wg_public_key = match gen_public_key(&vm_wg_private_key) {
                Ok(key) => key,
                Err(_) => return Err(DeployError::InternalServerError),
            };

            let mut wg_port_try: u8 = 0;
            let correct_wg_config = loop {
                if wg_port_try >= MAX_WG_PORT_RETRIES {
                    return Err(DeployError::InternalServerError);
                }
                wg_port_try += 1;

                let vm_ip = ctx.db.vms().allocate_vm_ip(_org.account_id()).await?;
                let vm_wg_port = ctx.db.vms().next_vm_wg_port(node_id).await?;

                let wg_config = VmWireGuardConfig {
                    private_key: vm_wg_private_key.to_string(),
                    public_key: vm_wg_public_key.to_string(),
                    ipv6_address: vm_ip.to_string(),
                    proxy_ipv6_address: VersConfig::proxy().wg_private_ip.to_string(),
                    proxy_public_key: VersConfig::proxy().wg_public_key.to_string(),
                    proxy_public_ip: VersConfig::proxy().public_ip.to_string(),
                    wg_port: vm_wg_port,
                };

                let db_insert = ctx
                    .db
                    .check_limits_and_insert_vm(
                        &_org,
                        VmInsertParams {
                            vm_id,
                            parent_commit_id: None,
                            grandparent_vm_id: None,
                            node_id,
                            ip: vm_ip,
                            wg_private_key: vm_wg_private_key.to_string(),
                            wg_public_key: vm_wg_public_key.to_string(),
                            wg_port: vm_wg_port,
                            owner_id: self.api_key.id(),
                            created_at: Utc::now(),
                            deleted_at: None,
                            vcpu_count: requirements.vcpu_count as i32,
                            mem_size_mib: requirements.mem_size_mib as i32,
                            labels: Some(labels.clone()),
                        },
                    )
                    .await;

                match db_insert {
                    Ok(_) => break wg_config,
                    Err(CheckAndInsertError::Db(db)) => return Err(db.into()),
                    Err(CheckAndInsertError::ResourceLimit(rl)) => {
                        return Err(DeployError::ResourceLimitExceeded(rl));
                    }
                    Err(CheckAndInsertError::NotUniqueNodeIdWgPortCombination) => continue,
                }
            };

            match ctx
                .proto()
                .new_vm(
                    &node,
                    VmCreateRequest {
                        vm_id: Some(vm_id),
                        vm_config: vm_config.clone(),
                        wireguard: correct_wg_config,
                        env_vars: None,
                    },
                    true, // wait_boot — we need the VM running before exec
                    self.request_id.as_deref(),
                )
                .await
            {
                Ok(_) => {
                    reservation.commit();
                    vm_created = true;
                    tracing::info!(%vm_id, node_id = %node_id, "Deploy: VM created");
                    break;
                }
                Err(provision_err) => {
                    if let Err(db_err) = ctx.db.vms().mark_deleted(&vm_id).await {
                        tracing::error!(%db_err, %vm_id, "Failed to clean up VM record");
                    }
                    last_provision_err = Some(provision_err);
                }
            }
        }

        if !vm_created {
            return match last_provision_err {
                Some(err) => Err(DeployError::VmProvisioningFailed(err)),
                None => Err(DeployError::InternalServerError),
            };
        }

        // ------------------------------------------------------------------
        // 8. Commit the fresh VM to get a root commit
        // ------------------------------------------------------------------
        let commit_id = Uuid::new_v4();
        ctx.db
            .commits()
            .insert(
                commit_id,
                None,                 // parent_vm_id
                None,                 // grandparent_commit_id
                self.api_key.id(),    // owner_id
                project_name.clone(), // name
                None,                 // description
                Utc::now(),           // created_at
                false,                // is_public
            )
            .await?;

        // ------------------------------------------------------------------
        // 9. Create project + project API key in vers_landing schema
        // ------------------------------------------------------------------
        let project_id = Uuid::new_v4();
        let settings = self.request.settings.as_ref();

        if let Err(e) = ctx
            .db
            .deploy()
            .insert_project(
                project_id,
                org_id,
                &project_name,
                commit_id,
                vm_id,
                self.api_key.user_id(),
                settings.and_then(|s| s.install_command.as_deref()),
                settings.and_then(|s| s.build_command.as_deref()),
                settings.and_then(|s| s.run_command.as_deref()),
                settings.and_then(|s| s.working_directory.as_deref()),
                Some(gh_repo.id),
            )
            .await
        {
            // Check for unique constraint violation (project name conflict)
            let err_str = e.to_string();
            if err_str.contains("unique") || err_str.contains("duplicate") {
                return Err(DeployError::ProjectNameConflict(project_name));
            }
            return Err(e.into());
        }

        // Link the GitHub repo to the project
        ctx.db
            .deploy()
            .update_project_github_repo(project_id, gh_repo.id)
            .await?;

        // Store the API key reference for the project
        ctx.db
            .deploy()
            .insert_project_api_key(project_id, self.api_key.id(), "")
            .await?;

        // ------------------------------------------------------------------
        // 10. Fetch user environment variables and build setup script
        // ------------------------------------------------------------------
        let env_vars = ctx
            .db
            .env_vars()
            .get_by_user_id(self.api_key.user_id())
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(%project_id, ?e, "Failed to fetch user env vars, continuing without them");
                HashMap::with_capacity(0)
            });

        let settings_ref = self.request.settings.as_ref().cloned().unwrap_or_default();
        let script = build_setup_script(&clone_url, &branch, &settings_ref, &env_vars);

        // Update status to cloning
        ctx.db
            .deploy()
            .update_project_clone_status(project_id, "cloning", None)
            .await?;

        // Get the node for exec
        let vm_entity = ctx.db.vms().get_by_id(vm_id).await?;
        let node_id = vm_entity
            .and_then(|v| v.node_id)
            .ok_or(DeployError::InternalServerError)?;
        let node = ctx
            .db
            .node()
            .get_by_id(&node_id)
            .await?
            .ok_or(DeployError::InternalServerError)?;

        // Execute the setup script in background via exec.
        // We write the script to a file and run it with bash to avoid
        // command-length limits and quoting issues.
        let write_result = ctx
            .proto()
            .vm_exec(
                &node,
                vm_id,
                VmExecRequest {
                    command: vec![
                        "bash".into(),
                        "-c".into(),
                        format!(
                            "cat > /tmp/vers_deploy.sh << 'VERS_DEPLOY_EOF'\n{script}\nVERS_DEPLOY_EOF\nchmod +x /tmp/vers_deploy.sh"
                        ),
                    ],
                    exec_id: None,
                    env: None,
                    working_dir: None,
                    stdin: None,
                    timeout_secs: Some(30),
                },
                self.request_id.as_deref(),
            )
            .await;

        if let Err(e) = write_result {
            tracing::error!(%project_id, ?e, "Failed to write deploy script to VM");
            ctx.db
                .deploy()
                .update_project_clone_status(
                    project_id,
                    "failed",
                    Some("Failed to write deploy script"),
                )
                .await?;
            return Ok(DeployResponse {
                project_id,
                vm_id,
                status: "failed".into(),
            });
        }

        // Launch the deploy script in the background
        let launch_result = ctx
            .proto()
            .vm_exec(
                &node,
                vm_id,
                VmExecRequest {
                    command: vec![
                        "bash".into(),
                        "-c".into(),
                        "nohup bash /tmp/vers_deploy.sh > /tmp/vers_deploy.log 2>&1 & echo launched".into(),
                    ],
                    exec_id: None,
                    env: None,
                    working_dir: None,
                    stdin: None,
                    timeout_secs: Some(30),
                },
                self.request_id.as_deref(),
            )
            .await;

        if let Err(e) = launch_result {
            tracing::error!(%project_id, ?e, "Failed to launch deploy script");
            ctx.db
                .deploy()
                .update_project_clone_status(
                    project_id,
                    "failed",
                    Some("Failed to launch deploy script"),
                )
                .await?;
        }

        Ok(DeployResponse {
            project_id,
            vm_id,
            status: "deploying".into(),
        })
    }
}

impl_error_response!(DeployError,
    DeployError::Db(_) => INTERNAL_SERVER_ERROR,
    DeployError::Forbidden => FORBIDDEN,
    DeployError::GitHubNotConfigured => NOT_IMPLEMENTED,
    DeployError::GitHubNotInstalled => FORBIDDEN,
    DeployError::RepoNotFound(_) => NOT_FOUND,
    DeployError::ProjectNameConflict(_) => CONFLICT,
    DeployError::CloneUrlFailed(_) => INTERNAL_SERVER_ERROR,
    DeployError::ChooseNodeError(_) => INTERNAL_SERVER_ERROR,
    DeployError::VmProvisioningFailed(_) => INTERNAL_SERVER_ERROR,
    DeployError::InternalServerError => INTERNAL_SERVER_ERROR,
    DeployError::BadRequest(_) => BAD_REQUEST,
    DeployError::ResourceLimitExceeded(_) => FORBIDDEN,
);
