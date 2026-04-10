use chrono::Utc;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, info};
use util::s3::{DeletePrefixError, delete_prefix};
use uuid::Uuid;
use vers_config::VersConfig;
use vers_pg::db::VersPg;

use crate::{
    action::{AuthzError, check_commit_access},
    db::{ApiKeyEntity, DB, DBError, VMCommitsRepository, VMsRepository},
};

#[derive(Debug, Clone)]
pub struct DeleteCommit {
    api_key: ApiKeyEntity,
    commit_id: Uuid,
    skip_storage_cleanup: bool,
}

impl DeleteCommit {
    pub fn new(api_key: ApiKeyEntity, commit_id: Uuid) -> Self {
        Self {
            api_key,
            commit_id,
            skip_storage_cleanup: false,
        }
    }

    #[cfg(any(test, feature = "integration-tests"))]
    pub fn skip_storage_cleanup_for_tests(mut self) -> Self {
        self.skip_storage_cleanup = true;
        self
    }
}

#[derive(Debug, Error)]
pub enum DeleteCommitError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("chelsea db error: {0}")]
    ChelseaDb(#[from] vers_pg::Error),
    #[error("commit not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("commit still has {0} active VM(s)")]
    ActiveVms(i64),
    #[error(transparent)]
    S3(#[from] DeletePrefixError),
}

impl DeleteCommit {
    pub async fn call(self, db: &DB, vers_pg: &Arc<VersPg>) -> Result<(), DeleteCommitError> {
        let commit = check_commit_access(db, &self.api_key, self.commit_id)
            .await
            .map_err(map_authz_error)?;

        if commit.owner_id != self.api_key.id() {
            return Err(DeleteCommitError::Forbidden);
        }

        let dependent_vm_count = db.vms().count_by_parent_commit(self.commit_id).await?;

        if dependent_vm_count > 0 {
            return Err(DeleteCommitError::ActiveVms(dependent_vm_count));
        }

        db.commits()
            .mark_deleted(self.commit_id, self.api_key.id(), Utc::now())
            .await?;

        if let Err(err) =
            mark_chelsea_commit_deleted(self.commit_id, self.api_key.id(), vers_pg).await
        {
            if let Err(rollback_err) = db.commits().clear_deleted(self.commit_id).await {
                error!(
                    commit_id = %self.commit_id,
                    %rollback_err,
                    "Failed to rollback orchestrator commit deletion after Chelsea error"
                );
            }
            return Err(err);
        }

        if !self.skip_storage_cleanup {
            cleanup_s3_objects(self.commit_id).await?;
        } else {
            info!(
                commit_id = %self.commit_id,
                "Skipping commit storage cleanup (test configuration)"
            );
        }

        info!(commit_id = %self.commit_id, "Deleted commit");
        Ok(())
    }
}

fn map_authz_error(err: AuthzError) -> DeleteCommitError {
    match err {
        AuthzError::CommitNotFound => DeleteCommitError::NotFound,
        AuthzError::Forbidden => DeleteCommitError::Forbidden,
        AuthzError::Db(db) => DeleteCommitError::Db(db),
        _ => DeleteCommitError::Forbidden,
    }
}

async fn cleanup_s3_objects(commit_id: Uuid) -> Result<(), DeleteCommitError> {
    let bucket = VersConfig::chelsea().aws_commit_bucket_name.clone();
    let prefix = format!("{commit_id}/");
    delete_prefix(&bucket, &prefix).await?;
    Ok(())
}

async fn mark_chelsea_commit_deleted(
    commit_id: Uuid,
    deleted_by: Uuid,
    vers_pg: &Arc<VersPg>,
) -> Result<(), DeleteCommitError> {
    vers_pg
        .chelsea
        .commit
        .mark_deleted(&commit_id, Some(deleted_by))
        .await?;
    Ok(())
}

impl_error_response!(DeleteCommitError,
    DeleteCommitError::Db(_) => INTERNAL_SERVER_ERROR,
    DeleteCommitError::ChelseaDb(_) => INTERNAL_SERVER_ERROR,
    DeleteCommitError::NotFound => NOT_FOUND,
    DeleteCommitError::Forbidden => FORBIDDEN,
    DeleteCommitError::ActiveVms(_) => CONFLICT,
    DeleteCommitError::S3(_) => INTERNAL_SERVER_ERROR,
);
