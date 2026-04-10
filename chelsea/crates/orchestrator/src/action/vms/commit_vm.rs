use chrono::{DateTime, Utc};
use dto_lib::chelsea_server2::vm::VmCommitResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    action::{Action, AuthzError, check_vm_access},
    db::{ApiKeyEntity, ChelseaNodeRepository, DBError, VMCommitsRepository},
    outbound::node_proto::HttpError,
};

#[derive(Debug, Clone)]
pub struct CommitVM {
    pub vm_id: Uuid,
    pub commit_id: Uuid,
    pub api_key: ApiKeyEntity,
    commit_name: String,
    commit_description: Option<String>,
    created_at: DateTime<Utc>,
    pub keep_paused: bool,
    pub skip_wait_boot: bool,
    pub request_id: Option<String>,
}
impl CommitVM {
    pub fn new(
        vm_id: Uuid,
        commit_id: Uuid,
        api_key: ApiKeyEntity,
        keep_paused: bool,
        skip_wait_boot: bool,
    ) -> Self {
        let created_at = Utc::now();
        Self {
            vm_id,
            commit_id,
            api_key: api_key,
            commit_name: format!("commit: of_vm={vm_id} timestamp={}", created_at),
            commit_description: None,
            created_at,
            keep_paused,
            skip_wait_boot,
            request_id: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.commit_name = name;
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.commit_description = description;
        self
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum CommitVMError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("vm not found")]
    VMNotFound,
    #[error("internal server error")]
    InternalServerError,

    #[error("The requested VM is not currently running (node_id is null); is it sleeping?")]
    NodeIdNull,

    #[error("Forbidden")]
    Forbidden,
    #[error("http error: {0:?}")]
    Http(#[from] HttpError),
}

impl Action for CommitVM {
    type Response = VmCommitResponse;
    type Error = CommitVMError;
    const ACTION_ID: &'static str = "vm.commit";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Check authorization
        let vm = check_vm_access(&ctx.db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => CommitVMError::VMNotFound,
                AuthzError::Forbidden | AuthzError::CommitNotFound | AuthzError::TagNotFound => {
                    CommitVMError::Forbidden
                }
                AuthzError::Db(db) => CommitVMError::Db(db),
            })?;

        // 2. Get the node where the VM is running
        let node_id = vm.node_id.ok_or(CommitVMError::NodeIdNull)?;
        let Some(node) = ctx.db.node().get_by_id(&node_id).await? else {
            return Err(CommitVMError::InternalServerError);
        };

        tracing::info!(
            vm_id = %self.vm_id,
            node_id = %node_id,
            node_ip = %node.ip_priv(),
            "Committing VM on Chelsea node"
        );

        // 3. Determine grandparent_commit_id: either the most recent commit of this VM,
        //    or if this is the first commit, the commit the VM was created from
        let grandparent_commit_id = match ctx.db.commits().get_latest_by_vm(self.vm_id).await? {
            Some(latest_commit) => {
                // There's already a commit for this VM, use it as the grandparent
                Some(latest_commit.id)
            }
            None => {
                // This is the first commit of this VM, use the commit it was created from
                vm.parent_commit_id.clone()
            }
        };

        // 4. Insert DB record FIRST to prevent race condition with ceph-gc
        //    If snapshot is created before DB record exists, GC could delete it
        //    before we insert the record, leaving a dangling reference
        ctx.db
            .commits()
            .insert(
                self.commit_id.clone(),
                Some(vm.id()),
                grandparent_commit_id,
                vm.owner_id(),
                self.commit_name.clone(),
                self.commit_description.clone(),
                self.created_at,
                false, // is_public: new commits are private by default
            )
            .await?;

        tracing::debug!(
            commit_id = %self.commit_id,
            "Inserted partial commit record before snapshot creation"
        );

        // 5. Call Chelsea node to commit the VM (creates Ceph snapshot)
        //    If this fails, we rollback the DB insert below
        let response = match ctx
            .proto()
            .vm_commit(
                &node,
                self.vm_id,
                self.commit_id.clone(),
                self.keep_paused,
                self.skip_wait_boot,
                self.request_id.as_deref(),
            )
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                // Rollback: hard delete the commit record since snapshot creation failed
                tracing::error!(
                    commit_id = %self.commit_id,
                    error = ?e,
                    "VM commit failed, rolling back DB insert"
                );
                if let Err(delete_err) = ctx.db.commits().hard_delete(self.commit_id).await {
                    tracing::error!(
                        commit_id = %self.commit_id,
                        error = ?delete_err,
                        "Failed to rollback commit record after VM commit failure"
                    );
                }
                return Err(e.into());
            }
        };

        tracing::info!(
            vm_id = %self.vm_id,
            commit_id = %response.commit_id,
            "Successfully committed VM"
        );

        Ok(response)
    }
}

impl_error_response!(CommitVMError,
    CommitVMError::Db(_) => INTERNAL_SERVER_ERROR,
    CommitVMError::VMNotFound => NOT_FOUND,
    CommitVMError::InternalServerError => INTERNAL_SERVER_ERROR,
    CommitVMError::NodeIdNull => CONFLICT,
    CommitVMError::Forbidden => FORBIDDEN,
    CommitVMError::Http(_) => INTERNAL_SERVER_ERROR,
);
