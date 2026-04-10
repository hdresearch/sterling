use chrono::Utc;
use orch_wg::{gen_private_key, gen_public_key};
use thiserror::Error;
use uuid::Uuid;
use vers_config::VersConfig;

use super::load_user_env_vars;
use crate::action::{
    self, Action, ActionError, AuthzError, ChooseNode, RechooseNodeError, VmRequirements,
    check_commit_read_access, check_resource_ownership,
};
use crate::db::{
    ApiKeyEntity, CheckAndInsertError, ChelseaNodeRepository, CommitTagsRepository, DBError,
    OrgsRepository, VMsRepository, VmInsertParams,
};
use crate::inbound::routes::controlplane::vm::NewVmResponse;
use crate::outbound::node_proto::HttpError;
use dto_lib::chelsea_server2::vm::{VmFromCommitRequest, VmWireGuardConfig};
use tracing::error;

/// Maximum retries for WireGuard port collisions on a single node.
const MAX_WG_PORT_RETRIES: u8 = 10;

/// Identifier for a commit - either by UUID, tag name, or repo_name:tag_name reference
#[derive(Debug, Clone)]
pub enum CommitIdentifier {
    CommitId(Uuid),
    TagName(String),
    /// Repository reference: repo_name:tag_name
    Ref {
        repo_name: String,
        tag_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct FromCommitVM {
    pub commit_identifier: CommitIdentifier,
    pub api_key: ApiKeyEntity,
    pub request_id: Option<String>,
}

impl FromCommitVM {
    pub fn new(commit_identifier: CommitIdentifier, api_key: ApiKeyEntity) -> Self {
        Self {
            commit_identifier,
            api_key,
            request_id: None,
        }
    }

    pub fn from_commit_id(commit_id: Uuid, api_key: ApiKeyEntity) -> Self {
        Self::new(CommitIdentifier::CommitId(commit_id), api_key)
    }

    pub fn from_tag_name(tag_name: String, api_key: ApiKeyEntity) -> Self {
        Self::new(CommitIdentifier::TagName(tag_name), api_key)
    }

    pub fn from_ref(repo_name: String, tag_name: String, api_key: ApiKeyEntity) -> Self {
        Self::new(
            CommitIdentifier::Ref {
                repo_name,
                tag_name,
            },
            api_key,
        )
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum FromCommitVMError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("chelsea db error: {0}")]
    DbChelsea(#[from] vers_pg::Error),
    #[error("http error: {0:?}")]
    Http(#[from] HttpError),

    #[error("rechoose node err: {0:?}")]
    RechooseError(#[from] RechooseNodeError),
    #[error("commit not found")]
    CommitNotFound,
    #[error("tag not found")]
    TagNotFound,
    #[error("internal error")]
    InternalError,

    #[error("forbidden")]
    Forbidden,

    #[error("{0}")]
    ResourceLimitExceeded(#[from] super::ResourceLimitError),
}

impl From<CheckAndInsertError> for FromCommitVMError {
    fn from(e: CheckAndInsertError) -> Self {
        match e {
            CheckAndInsertError::Db(db) => FromCommitVMError::Db(db),
            CheckAndInsertError::ResourceLimit(rl) => FromCommitVMError::ResourceLimitExceeded(rl),
            CheckAndInsertError::NotUniqueNodeIdWgPortCombination => {
                FromCommitVMError::InternalError
            }
        }
    }
}

impl Action for FromCommitVM {
    type Response = NewVmResponse;
    type Error = FromCommitVMError;
    const ACTION_ID: &'static str = "vm.from_commit";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Resolve commit_id from either direct ID or tag name
        let commit_id = match self.commit_identifier {
            CommitIdentifier::CommitId(id) => id,
            CommitIdentifier::TagName(tag_name) => {
                // Look up tag by (org_id, name) for authenticated user's organization
                let tag = ctx
                    .db
                    .commit_tags()
                    .get_by_name(self.api_key.org_id(), &tag_name)
                    .await?
                    .ok_or(FromCommitVMError::TagNotFound)?;

                // Check org-level access to tag
                check_resource_ownership(&ctx.db, &self.api_key, tag.owner_id)
                    .await
                    .map_err(|e| match e {
                        AuthzError::Forbidden => FromCommitVMError::Forbidden,
                        AuthzError::Db(db) => FromCommitVMError::Db(db),
                        AuthzError::VmNotFound
                        | AuthzError::CommitNotFound
                        | AuthzError::TagNotFound => FromCommitVMError::Forbidden,
                    })?;

                tracing::info!(
                    tag_name = %tag_name,
                    tag_id = %tag.id,
                    commit_id = %tag.commit_id,
                    "Resolved tag to commit"
                );

                tag.commit_id
            }
            CommitIdentifier::Ref {
                repo_name,
                tag_name,
            } => {
                // Resolve repo_name:tag_name to a commit
                let tag = ctx
                    .db
                    .commit_tags()
                    .resolve_ref(self.api_key.org_id(), &repo_name, &tag_name)
                    .await?
                    .ok_or(FromCommitVMError::TagNotFound)?;

                check_resource_ownership(&ctx.db, &self.api_key, tag.owner_id)
                    .await
                    .map_err(|e| match e {
                        AuthzError::Forbidden => FromCommitVMError::Forbidden,
                        AuthzError::Db(db) => FromCommitVMError::Db(db),
                        _ => FromCommitVMError::Forbidden,
                    })?;

                tracing::info!(
                    repo_name = %repo_name,
                    tag_name = %tag_name,
                    commit_id = %tag.commit_id,
                    "Resolved repo ref to commit"
                );

                tag.commit_id
            }
        };

        let commit = check_commit_read_access(&ctx.db, &self.api_key, commit_id)
            .await
            .map_err(|e| match e {
                AuthzError::CommitNotFound => FromCommitVMError::CommitNotFound,
                AuthzError::Forbidden => FromCommitVMError::Forbidden,
                AuthzError::Db(db) => FromCommitVMError::Db(db),
                _ => FromCommitVMError::Forbidden,
            })?;

        let Some(org) = ctx.db.orgs().get_by_id(self.api_key.org_id()).await? else {
            return Err(FromCommitVMError::InternalError);
        };
        let env_vars = load_user_env_vars(ctx, self.api_key.user_id()).await?;

        // Get the parent VM's node_id for sticky placement
        let preferred_node_id = if let Some(parent_vm_id) = commit.parent_vm_id {
            match ctx.db.vms().get_by_id(parent_vm_id).await {
                Ok(Some(parent_vm)) => parent_vm.node_id,
                Ok(None) => None,
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to fetch parent VM for sticky placement");
                    None
                }
            }
        } else {
            None
        };

        let commit_metadata = ctx.vers_pg.chelsea.commit.fetch_by_id(&commit_id).await?;

        let requirements =
            VmRequirements::new(commit_metadata.vcpu_count, commit_metadata.mem_size_mib);

        // Resource limits are enforced atomically inside check_limits_and_insert_vm.

        let choose_node = match preferred_node_id {
            Some(pref_id) => {
                ChooseNode::with_preferred_node(pref_id).with_requirements(requirements)
            }
            None => ChooseNode::new().with_requirements(requirements),
        };

        let mut candidates = match action::call(choose_node).await {
            Ok(c) => c,
            Err(ActionError::Error(err)) => return Err(err.into()),
            Err(_) => return Err(FromCommitVMError::InternalError),
        };

        let mut last_err: Option<HttpError> = None;

        while let Some(reservation) = candidates.next_node() {
            let node_id = reservation.node_id();

            let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
                continue;
            };

            let vm_id = Uuid::new_v4();
            let vm_wg_private_key = gen_private_key();
            let vm_wg_public_key = match gen_public_key(&vm_wg_private_key) {
                Ok(key) => key,
                Err(_) => {
                    tracing::error!("Failed to generate WireGuard public key");
                    return Err(FromCommitVMError::InternalError);
                }
            };

            let mut wg_port_try: u8 = 0;
            let correct_wg_config = loop {
                if wg_port_try >= MAX_WG_PORT_RETRIES {
                    tracing::error!(node_id = %node_id, "Exhausted WG port retries");
                    return Err(FromCommitVMError::InternalError);
                }
                wg_port_try += 1;

                let vm_ip = ctx.db.vms().allocate_vm_ip(org.account_id()).await?;
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

                tracing::info!(
                    node_id = %node_id,
                    account_id = %org.account_id(),
                    vm_ip = %vm_ip,
                    "Provisioning VM from commit on node"
                );

                let db_insert = ctx
                    .db
                    .check_limits_and_insert_vm(
                        &org,
                        VmInsertParams {
                            vm_id,
                            parent_commit_id: Some(commit.id),
                            grandparent_vm_id: commit.parent_vm_id,
                            node_id: *node.id(),
                            ip: vm_ip,
                            wg_private_key: vm_wg_private_key.to_string(),
                            wg_public_key: vm_wg_public_key.to_string(),
                            wg_port: vm_wg_port,
                            owner_id: self.api_key.id(),
                            created_at: Utc::now(),
                            deleted_at: None,
                            vcpu_count: requirements.vcpu_count as i32,
                            mem_size_mib: requirements.mem_size_mib as i32,
                            labels: None,
                        },
                    )
                    .await;

                match db_insert {
                    Ok(_) => break wg_config,
                    Err(CheckAndInsertError::Db(db)) => return Err(db.into()),
                    Err(CheckAndInsertError::ResourceLimit(rl)) => {
                        return Err(FromCommitVMError::ResourceLimitExceeded(rl));
                    }
                    Err(CheckAndInsertError::NotUniqueNodeIdWgPortCombination) => continue,
                }
            };

            match ctx
                .proto()
                .vm_from_commit(
                    &node,
                    VmFromCommitRequest {
                        vm_id: Some(vm_id),
                        commit_id,
                        wireguard: correct_wg_config,
                        env_vars: env_vars.clone(),
                    },
                    self.request_id.as_deref(),
                )
                .await
            {
                Ok(_) => {
                    reservation.commit();
                    tracing::info!(
                        %vm_id, commit_id = %commit_id, node_id = %node.id(),
                        "Successfully restored VM from commit"
                    );
                    return Ok(NewVmResponse {
                        vm_id: vm_id.to_string(),
                    });
                }
                Err(provision_err) => {
                    if let Err(db_err) = ctx.db.vms().mark_deleted(&vm_id).await {
                        error!(%db_err, %vm_id, "Failed to clean up VM record after provisioning failure");
                    }
                    tracing::warn!(
                        node_id = %node_id,
                        remaining = candidates.remaining(),
                        err = ?provision_err,
                        "Provisioning from commit failed, trying next candidate"
                    );
                    last_err = Some(provision_err);
                }
            }
        }

        tracing::error!(
            commit_id = %commit_id,
            "VM restore from commit failed on all candidate nodes"
        );
        match last_err {
            Some(err) => Err(FromCommitVMError::Http(err)),
            None => Err(FromCommitVMError::InternalError),
        }
    }
}

impl_error_response!(FromCommitVMError,
    FromCommitVMError::Db(_) => INTERNAL_SERVER_ERROR,
    FromCommitVMError::DbChelsea(_) => INTERNAL_SERVER_ERROR,
    FromCommitVMError::Http(_) => INTERNAL_SERVER_ERROR,
    FromCommitVMError::RechooseError(_) => INTERNAL_SERVER_ERROR,
    FromCommitVMError::CommitNotFound => NOT_FOUND,
    FromCommitVMError::TagNotFound => NOT_FOUND,
    FromCommitVMError::InternalError => INTERNAL_SERVER_ERROR,
    FromCommitVMError::Forbidden => FORBIDDEN,
    FromCommitVMError::ResourceLimitExceeded(_) => FORBIDDEN,
);
