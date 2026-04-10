use chrono::Utc;
use orch_wg::{gen_private_key, gen_public_key};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;
use vers_config::VersConfig;

use super::load_user_env_vars;
use crate::action::{
    self, Action, ActionContext, ActionError, AuthzError, ChooseNode, CommitVM, CommitVMError,
    RechooseNodeError, VmRequirements, check_commit_read_access_entity, check_resource_ownership,
    check_vm_access,
};
use crate::db::{
    ApiKeyEntity, CheckAndInsertError, ChelseaNodeRepository, CommitTagsRepository, DBError,
    OrganizationEntity, OrgsRepository, VMCommitsRepository, VMsRepository, VmCommitEntity,
    VmInsertParams,
};
use crate::inbound::routes::controlplane::vm::{NewVmResponse, NewVmsResponse};
use crate::outbound::node_proto::HttpError;
use dto_lib::chelsea_server2::vm::{VmFromCommitRequest, VmWireGuardConfig};
use tracing::error;

/// Maximum retries for WireGuard port collisions on a single node.
const MAX_WG_PORT_RETRIES: u8 = 10;

#[derive(Debug, Clone)]
pub enum BranchBy {
    Commit {
        commit: VmCommitEntity,
    },
    Vm {
        vm_id: Uuid,
        commit_id: Uuid,
        keep_paused: bool,
        skip_wait_boot: bool,
    },
    Tag {
        tag_name: String,
    },
    /// Branch from a repository reference (repo_name:tag_name)
    Ref {
        repo_name: String,
        tag_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct Branch {
    by: BranchBy,
    num: u8,
    api_key: ApiKeyEntity,
    #[allow(unused)]
    alias: Option<String>,
    request_id: Option<String>,
    /// When true, skip the commit-level access check. Used by fork_repository
    /// which has already verified access through the public repo layer.
    skip_access_check: bool,
}

#[derive(Debug, Error)]
pub enum BranchVMError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("chelsea db error: {0}")]
    DbChelsea(#[from] vers_pg::Error),
    #[error("http-error: {0:?}")]
    Http(#[from] HttpError),

    #[error("provided commit not found")]
    CommitNotFound,
    #[error("tag not found")]
    TagNotFound,
    #[error("parent vm not found")]
    ParentVMNotFound,
    #[error("Forbidden")]
    Forbidden,
    #[error("internal server error")]
    InternalServerError,

    #[error("error choosing node error")]
    ChooseNodeError(#[from] RechooseNodeError),

    #[error("error committing vm: {0}")]
    CommitVm(#[from] ActionError<CommitVMError>),

    #[error("{0}")]
    ResourceLimitExceeded(#[from] super::ResourceLimitError),
}

impl From<CheckAndInsertError> for BranchVMError {
    fn from(e: CheckAndInsertError) -> Self {
        match e {
            CheckAndInsertError::Db(db) => BranchVMError::Db(db),
            CheckAndInsertError::ResourceLimit(rl) => BranchVMError::ResourceLimitExceeded(rl),
            CheckAndInsertError::NotUniqueNodeIdWgPortCombination => {
                BranchVMError::InternalServerError
            }
        }
    }
}

impl Action for Branch {
    type Response = NewVmsResponse;
    type Error = NewVmsResponse;
    const ACTION_ID: &'static str = "vm.branch";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        let Some(org) = ctx
            .db
            .orgs()
            .get_by_id(self.api_key.org_id())
            .await
            .map_err(|err| NewVmsResponse::with_error(vec![], BranchVMError::from(err)))?
        else {
            return Err(NewVmsResponse::with_error(vec![], BranchVMError::Forbidden));
        };
        let env_vars = load_user_env_vars(ctx, self.api_key.user_id())
            .await
            .map_err(|err| NewVmsResponse::with_error(vec![], BranchVMError::from(err)))?;

        let commit = match self.by {
            BranchBy::Vm {
                vm_id,
                commit_id,
                keep_paused,
                skip_wait_boot,
            } => {
                let _vm = check_vm_access(&ctx.db, &self.api_key, vm_id)
                    .await
                    .map_err(|e| {
                        let err = match e {
                            AuthzError::VmNotFound => BranchVMError::ParentVMNotFound,
                            AuthzError::Forbidden
                            | AuthzError::CommitNotFound
                            | AuthzError::TagNotFound => BranchVMError::Forbidden,
                            AuthzError::Db(db) => BranchVMError::Db(db),
                        };
                        NewVmsResponse::with_error(vec![], err)
                    })?;

                action::call(
                    CommitVM::new(
                        vm_id,
                        commit_id,
                        self.api_key.clone(),
                        keep_paused,
                        skip_wait_boot,
                    )
                    .with_request_id(self.request_id.clone()),
                )
                .await
                .map_err(|err| NewVmsResponse::with_error(vec![], BranchVMError::from(err)))?;

                let Some(commit) =
                    ctx.db.commits().get_by_id(commit_id).await.map_err(|db| {
                        NewVmsResponse::with_error(vec![], BranchVMError::from(db))
                    })?
                else {
                    return Err(NewVmsResponse::with_error(
                        vec![],
                        BranchVMError::CommitNotFound,
                    ));
                };
                commit
            }
            BranchBy::Commit { ref commit } => {
                if !self.skip_access_check {
                    check_commit_read_access_entity(&ctx.db, &self.api_key, commit)
                        .await
                        .map_err(|e| {
                            let err = match e {
                                AuthzError::Forbidden
                                | AuthzError::VmNotFound
                                | AuthzError::CommitNotFound
                                | AuthzError::TagNotFound => BranchVMError::Forbidden,
                                AuthzError::Db(db) => BranchVMError::Db(db),
                            };
                            NewVmsResponse::with_error(vec![], err)
                        })?;
                }

                commit.clone()
            }
            BranchBy::Tag { ref tag_name } => {
                // Look up tag by (org_id, name)
                let tag = ctx
                    .db
                    .commit_tags()
                    .get_by_name(self.api_key.org_id(), tag_name)
                    .await
                    .map_err(|db| NewVmsResponse::with_error(vec![], BranchVMError::from(db)))?
                    .ok_or_else(|| {
                        NewVmsResponse::with_error(vec![], BranchVMError::TagNotFound)
                    })?;

                // Check org-level access to tag
                check_resource_ownership(&ctx.db, &self.api_key, tag.owner_id)
                    .await
                    .map_err(|e| {
                        let err = match e {
                            AuthzError::Forbidden
                            | AuthzError::VmNotFound
                            | AuthzError::CommitNotFound
                            | AuthzError::TagNotFound => BranchVMError::Forbidden,
                            AuthzError::Db(db) => BranchVMError::Db(db),
                        };
                        NewVmsResponse::with_error(vec![], err)
                    })?;

                tracing::info!(
                    tag_name = %tag_name,
                    tag_id = %tag.id,
                    commit_id = %tag.commit_id,
                    "Resolved tag to commit for branching"
                );

                // Fetch the commit
                let Some(commit) = ctx
                    .db
                    .commits()
                    .get_by_id(tag.commit_id)
                    .await
                    .map_err(|db| NewVmsResponse::with_error(vec![], BranchVMError::from(db)))?
                else {
                    return Err(NewVmsResponse::with_error(
                        vec![],
                        BranchVMError::CommitNotFound,
                    ));
                };
                commit
            }
            BranchBy::Ref {
                ref repo_name,
                ref tag_name,
            } => {
                // Resolve repo_name:tag_name to a commit
                let tag = ctx
                    .db
                    .commit_tags()
                    .resolve_ref(self.api_key.org_id(), repo_name, tag_name)
                    .await
                    .map_err(|db| NewVmsResponse::with_error(vec![], BranchVMError::from(db)))?
                    .ok_or_else(|| {
                        NewVmsResponse::with_error(vec![], BranchVMError::TagNotFound)
                    })?;

                check_resource_ownership(&ctx.db, &self.api_key, tag.owner_id)
                    .await
                    .map_err(|e| {
                        let err = match e {
                            AuthzError::Forbidden
                            | AuthzError::VmNotFound
                            | AuthzError::CommitNotFound
                            | AuthzError::TagNotFound => BranchVMError::Forbidden,
                            AuthzError::Db(db) => BranchVMError::Db(db),
                        };
                        NewVmsResponse::with_error(vec![], err)
                    })?;

                tracing::info!(
                    repo_name = %repo_name,
                    tag_name = %tag_name,
                    commit_id = %tag.commit_id,
                    "Resolved repo ref to commit for branching"
                );

                let Some(commit) = ctx
                    .db
                    .commits()
                    .get_by_id(tag.commit_id)
                    .await
                    .map_err(|db| NewVmsResponse::with_error(vec![], BranchVMError::from(db)))?
                else {
                    return Err(NewVmsResponse::with_error(
                        vec![],
                        BranchVMError::CommitNotFound,
                    ));
                };
                commit
            }
        };

        let mut created_vms = vec![];

        for _ in 0..self.num {
            match self
                .branch_by_commit(&commit, ctx, org.clone(), env_vars.clone(), None)
                .await
            {
                Ok(value) => {
                    created_vms.push(value);
                }
                Err(err) => {
                    return Err(NewVmsResponse::with_error(created_vms, err));
                }
            };
        }

        return Ok(NewVmsResponse {
            vms: created_vms,
            error: None,
        });
    }
}

impl Branch {
    fn new(by: BranchBy, api_key: ApiKeyEntity, alias: Option<String>, num: Option<u8>) -> Self {
        Self {
            api_key,
            by,
            alias,
            num: num.unwrap_or(1),
            request_id: None,
            skip_access_check: false,
        }
    }
    pub fn by_commit(
        api_key: ApiKeyEntity,
        commit: VmCommitEntity,
        alias: Option<String>,
        num: Option<u8>,
    ) -> Self {
        Self::new(BranchBy::Commit { commit }, api_key, alias, num)
    }
    pub fn by_vm(
        api_key: ApiKeyEntity,
        vm_id: Uuid,
        commit_id: Uuid,
        alias: Option<String>,
        keep_paused: bool,
        skip_wait_boot: bool,
        num: Option<u8>,
    ) -> Self {
        Self::new(
            BranchBy::Vm {
                vm_id,
                commit_id,
                keep_paused,
                skip_wait_boot,
            },
            api_key,
            alias,
            num,
        )
    }

    pub fn by_tag(
        api_key: ApiKeyEntity,
        tag_name: String,
        alias: Option<String>,
        num: Option<u8>,
    ) -> Self {
        Self::new(BranchBy::Tag { tag_name }, api_key, alias, num)
    }

    pub fn by_ref(
        api_key: ApiKeyEntity,
        repo_name: String,
        tag_name: String,
        alias: Option<String>,
        num: Option<u8>,
    ) -> Self {
        Self::new(
            BranchBy::Ref {
                repo_name,
                tag_name,
            },
            api_key,
            alias,
            num,
        )
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    /// Skip commit-level access checks. Use only when access has already been
    /// verified through another layer (e.g. public repository resolution).
    pub fn with_skip_access_check(mut self) -> Self {
        self.skip_access_check = true;
        self
    }

    async fn branch_by_commit(
        &self,
        commit_to_branch: &VmCommitEntity,
        ctx: &ActionContext,
        org: OrganizationEntity,
        env_vars: Option<HashMap<String, String>>,
        labels: Option<HashMap<String, String>>,
    ) -> Result<NewVmResponse, BranchVMError> {
        let vm_ip = ctx.db.vms().allocate_vm_ip(org.account_id()).await?;

        tracing::info!(
            %commit_to_branch.id,
            vm_ip = %vm_ip,
            "Branching VM on Chelsea node with allocated IPv6"
        );

        // Get the parent VM's node_id for sticky placement
        let preferred_node_id = if let Some(parent_vm_id) = commit_to_branch.parent_vm_id {
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

        let commit_metadata = ctx
            .vers_pg
            .chelsea
            .commit
            .fetch_by_id(&commit_to_branch.id)
            .await?;

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
            Err(_) => return Err(BranchVMError::InternalServerError),
        };

        let mut last_err: Option<HttpError> = None;

        while let Some(reservation) = candidates.next_node() {
            let node_id = reservation.node_id();

            let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
                tracing::error!(node_id = %node_id, "Node not found from ChooseNode result");
                continue;
            };

            let vm_id = Uuid::new_v4();
            let vm_wg_private_key = gen_private_key();
            let vm_wg_public_key = match gen_public_key(&vm_wg_private_key) {
                Ok(key) => key,
                Err(_) => {
                    tracing::error!("Failed to generate WireGuard public key");
                    return Err(BranchVMError::InternalServerError);
                }
            };

            let mut wg_port_try: u8 = 0;
            let correct_wg_config = loop {
                if wg_port_try >= MAX_WG_PORT_RETRIES {
                    tracing::error!(node_id = %node_id, "Exhausted WG port retries");
                    return Err(BranchVMError::InternalServerError);
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
                    "Provisioning branched VM on node"
                );

                let db_insert = ctx
                    .db
                    .check_limits_and_insert_vm(
                        &org,
                        VmInsertParams {
                            vm_id,
                            parent_commit_id: Some(commit_to_branch.id),
                            grandparent_vm_id: commit_to_branch.parent_vm_id,
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
                            labels: labels.clone(),
                        },
                    )
                    .await;

                match db_insert {
                    Ok(_) => break wg_config,
                    Err(CheckAndInsertError::Db(db)) => return Err(db.into()),
                    Err(CheckAndInsertError::ResourceLimit(rl)) => {
                        return Err(BranchVMError::ResourceLimitExceeded(rl));
                    }
                    Err(CheckAndInsertError::NotUniqueNodeIdWgPortCombination) => continue,
                }
            };

            let commit_id = commit_to_branch.id;
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
                    tracing::info!(%vm_id, vm_ip = %vm_ip, "Chelsea created branched VM");
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
                        "Branch provisioning failed, trying next candidate"
                    );
                    last_err = Some(provision_err);
                }
            }
        }

        tracing::error!(
            commit_id = %commit_to_branch.id,
            "Branch VM provisioning failed on all candidate nodes"
        );
        match last_err {
            Some(err) => Err(BranchVMError::from(err)),
            None => Err(BranchVMError::InternalServerError),
        }
    }
}

impl_error_response!(BranchVMError,
    BranchVMError::Db(_) => INTERNAL_SERVER_ERROR,
    BranchVMError::DbChelsea(_) => INTERNAL_SERVER_ERROR,
    BranchVMError::Http(_) => INTERNAL_SERVER_ERROR,
    BranchVMError::CommitNotFound => NOT_FOUND,
    BranchVMError::TagNotFound => NOT_FOUND,
    BranchVMError::ParentVMNotFound => NOT_FOUND,
    BranchVMError::Forbidden => FORBIDDEN,
    BranchVMError::InternalServerError => INTERNAL_SERVER_ERROR,
    BranchVMError::ChooseNodeError(_) => INTERNAL_SERVER_ERROR,
    BranchVMError::CommitVm(_) => INTERNAL_SERVER_ERROR,
    BranchVMError::ResourceLimitExceeded(_) => FORBIDDEN,
);
