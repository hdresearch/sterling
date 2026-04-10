use chrono::Utc;
use orch_wg::{gen_private_key, gen_public_key};
use thiserror::Error;
use uuid::Uuid;
use vers_config::VersConfig;

use super::load_user_env_vars;
use crate::action::{self, Action, ActionError, ChooseNode, RechooseNodeError, VmRequirements};
use crate::db::{
    ApiKeyEntity, BaseImagesRepository, CheckAndInsertError, ChelseaNodeRepository, DBError,
    OrgsRepository, VMsRepository, VmInsertParams,
};
use crate::inbound::routes::controlplane::vm::NewVmResponse;
use crate::outbound::node_proto::HttpError;
use dto_lib::chelsea_server2::vm::{VmCreateRequest, VmCreateVmConfig, VmWireGuardConfig};
use tracing::{Instrument, error};

/// Maximum retries for WireGuard port collisions on a single node.
const MAX_WG_PORT_RETRIES: u8 = 10;

#[derive(Debug, Clone)]
pub struct NewRootVM {
    request: VmCreateVmConfig,
    api_key: ApiKeyEntity,
    wait_boot: bool,
    request_id: Option<String>,
}

impl NewRootVM {
    pub fn new(request: VmCreateVmConfig, api_key: ApiKeyEntity, wait_boot: bool) -> Self {
        Self {
            request,
            api_key,
            wait_boot,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum NewRootVMError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("internal server error")]
    InternalServerError,
    #[error("no available nodes")]
    ChooseNodeError(#[from] RechooseNodeError),
    #[error("failed to provision VM on node: {0}")]
    VmProvisioningFailed(#[from] HttpError),
    #[error("image not found: {0}")]
    ImageNotFound(String),

    #[error("Forbidden")]
    Forbidden,

    #[error("{0}")]
    ResourceLimitExceeded(#[from] super::ResourceLimitError),
}

impl From<CheckAndInsertError> for NewRootVMError {
    fn from(e: CheckAndInsertError) -> Self {
        match e {
            CheckAndInsertError::Db(db) => NewRootVMError::Db(db),
            CheckAndInsertError::ResourceLimit(rl) => NewRootVMError::ResourceLimitExceeded(rl),
            CheckAndInsertError::NotUniqueNodeIdWgPortCombination => {
                // Caller handles this with a retry; this shouldn't propagate.
                NewRootVMError::InternalServerError
            }
        }
    }
}

impl Action for NewRootVM {
    type Response = NewVmResponse;
    type Error = NewRootVMError;
    const ACTION_ID: &'static str = "vm.new_root";

    #[tracing::instrument(skip_all, fields(action = "vm.new_root"))]
    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        let org = ctx
            .db
            .orgs()
            .get_by_id(self.api_key.org_id())
            .instrument(tracing::info_span!("new_root.org_lookup"))
            .await?;
        let Some(org) = org else {
            return Err(NewRootVMError::Forbidden);
        };
        let env_vars = load_user_env_vars(ctx, self.api_key.user_id()).await?;

        // Resolve image name
        let rbd_image_name = if let Some(ref user_image_name) = self.request.image_name {
            if user_image_name == "default" {
                "default".to_string()
            } else {
                let image = ctx
                    .db
                    .base_images()
                    .get_visible_by_name(self.api_key.id(), user_image_name)
                    .await?;

                match image {
                    Some(img) => img.rbd_image_name,
                    None => {
                        return Err(NewRootVMError::ImageNotFound(user_image_name.clone()));
                    }
                }
            }
        } else {
            "default".to_string()
        };

        let requirements =
            VmRequirements::from_optional(self.request.vcpu_count, self.request.mem_size_mib);

        // Get ranked candidate nodes — best first.
        let mut candidates =
            match async { action::call(ChooseNode::new().with_requirements(requirements)).await }
                .instrument(tracing::info_span!("new_root.choose_node"))
                .await
            {
                Ok(c) => c,
                Err(ActionError::Error(err)) => return Err(err.into()),
                Err(_) => return Err(NewRootVMError::InternalServerError),
            };

        // Try each candidate node in order until provisioning succeeds.
        let mut last_err: Option<HttpError> = None;

        while let Some(reservation) = candidates.next_node() {
            let Some(node) = ctx.db.node().get_by_id(&reservation.node_id()).await? else {
                continue;
            };

            let node_id = *node.id();

            // Fresh VM identity per attempt.
            let vm_id = Uuid::new_v4();
            let vm_wg_private_key = gen_private_key();
            let vm_wg_public_key = match gen_public_key(&vm_wg_private_key) {
                Ok(key) => key,
                Err(_) => {
                    tracing::error!("Failed to generate WireGuard public key");
                    return Err(NewRootVMError::InternalServerError);
                }
            };

            // WG port retry loop for this node.
            let mut wg_port_try: u8 = 0;
            let correct_wg_config = loop {
                if wg_port_try >= MAX_WG_PORT_RETRIES {
                    tracing::error!(node_id = %node_id, "Exhausted WG port retries");
                    return Err(NewRootVMError::InternalServerError);
                }
                wg_port_try += 1;

                let vm_ip = ctx
                    .db
                    .vms()
                    .allocate_vm_ip(org.account_id())
                    .instrument(tracing::info_span!("new_root.allocate_vm_ip"))
                    .await?;
                let vm_wg_port = ctx
                    .db
                    .vms()
                    .next_vm_wg_port(node_id)
                    .instrument(tracing::info_span!("new_root.next_vm_wg_port"))
                    .await?;

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
                    "Provisioning new root VM on node"
                );

                let db_insert = {
                    ctx.db
                        .check_limits_and_insert_vm(
                            &org,
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
                                labels: self.request.labels.clone(),
                            },
                        )
                        .instrument(tracing::info_span!("new_root.check_and_insert_vm"))
                        .await
                };

                match db_insert {
                    Ok(_) => break wg_config,
                    Err(CheckAndInsertError::Db(db)) => return Err(db.into()),
                    Err(CheckAndInsertError::ResourceLimit(rl)) => {
                        return Err(NewRootVMError::ResourceLimitExceeded(rl));
                    }
                    Err(CheckAndInsertError::NotUniqueNodeIdWgPortCombination) => continue,
                }
            };

            let vm_config = VmCreateVmConfig {
                image_name: Some(rbd_image_name.clone()),
                ..self.request.clone()
            };

            match ctx
                .proto()
                .new_vm(
                    &node,
                    VmCreateRequest {
                        vm_id: Some(vm_id),
                        vm_config,
                        wireguard: correct_wg_config,
                        env_vars: env_vars.clone(),
                    },
                    self.wait_boot,
                    self.request_id.as_deref(),
                )
                .instrument(tracing::info_span!("new_root.proto_new_vm"))
                .await
            {
                Ok(_) => {
                    reservation.commit();
                    tracing::info!(%vm_id, node_id = %node_id, "Successfully created root VM");
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
                        "Provisioning failed on node, trying next candidate"
                    );
                    last_err = Some(provision_err);
                    // reservation drops here → pending resources released
                }
            }
        }

        // All candidates exhausted.
        tracing::error!("VM provisioning failed on all candidate nodes");
        match last_err {
            Some(err) => Err(NewRootVMError::VmProvisioningFailed(err)),
            None => Err(NewRootVMError::InternalServerError),
        }
    }
}

impl_error_response!(NewRootVMError,
    NewRootVMError::Db(_) => INTERNAL_SERVER_ERROR,
    NewRootVMError::InternalServerError => INTERNAL_SERVER_ERROR,
    NewRootVMError::ChooseNodeError(_) => INTERNAL_SERVER_ERROR,
    NewRootVMError::VmProvisioningFailed(_) => INTERNAL_SERVER_ERROR,
    NewRootVMError::ImageNotFound(_) => NOT_FOUND,
    NewRootVMError::Forbidden => FORBIDDEN,
    NewRootVMError::ResourceLimitExceeded(_) => FORBIDDEN,
);
