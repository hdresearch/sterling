use thiserror::Error;
use uuid::Uuid;

use crate::db::{
    ApiKeyEntity, ApiKeysRepository, CommitTagEntity, CommitTagsRepository, DB, DBError,
    VMCommitsRepository, VMsRepository, VmCommitEntity, VmEntity,
};

/// Errors that can occur during authorization checks.
#[derive(Debug, Error)]
pub enum AuthzError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("vm not found")]
    VmNotFound,
    #[error("commit not found")]
    CommitNotFound,
    #[error("tag not found")]
    TagNotFound,
    #[error("forbidden")]
    Forbidden,
}

/// Check if the given API key has access to the specified VM.
///
/// Returns the VM entity if access is granted.
///
/// # Errors
/// - `AuthzError::VmNotFound` if the VM does not exist
/// - `AuthzError::Forbidden` if the API key's org doesn't match the VM owner's org
/// - `AuthzError::Db` for database errors
pub async fn check_vm_access(
    db: &DB,
    api_key: &ApiKeyEntity,
    vm_id: Uuid,
) -> Result<VmEntity, AuthzError> {
    let vm = db
        .vms()
        .get_by_id(vm_id)
        .await?
        .ok_or(AuthzError::VmNotFound)?;

    check_resource_ownership(db, api_key, vm.owner_id()).await?;

    Ok(vm)
}

/// Check if the given API key has access to the specified commit.
///
/// Returns the commit entity if access is granted.
///
/// # Errors
/// - `AuthzError::CommitNotFound` if the commit does not exist
/// - `AuthzError::Forbidden` if the API key's org doesn't match the commit owner's org
/// - `AuthzError::Db` for database errors
pub async fn check_commit_access(
    db: &DB,
    api_key: &ApiKeyEntity,
    commit_id: Uuid,
) -> Result<VmCommitEntity, AuthzError> {
    let commit = db
        .commits()
        .get_by_id(commit_id)
        .await?
        .ok_or(AuthzError::CommitNotFound)?;

    check_resource_ownership(db, api_key, commit.owner_id).await?;

    Ok(commit)
}

/// Check if the given API key has read access to the specified commit.
///
/// Unlike `check_commit_access`, this allows access to public commits
/// regardless of org membership. Use this for read/restore/branch operations.
///
/// # Errors
/// - `AuthzError::CommitNotFound` if the commit does not exist
/// - `AuthzError::Forbidden` if the commit is private and the API key's org doesn't match
/// - `AuthzError::Db` for database errors
pub async fn check_commit_read_access(
    db: &DB,
    api_key: &ApiKeyEntity,
    commit_id: Uuid,
) -> Result<VmCommitEntity, AuthzError> {
    let commit = db
        .commits()
        .get_by_id(commit_id)
        .await?
        .ok_or(AuthzError::CommitNotFound)?;

    if commit.is_public {
        return Ok(commit);
    }

    check_resource_ownership(db, api_key, commit.owner_id).await?;

    Ok(commit)
}

/// Check if the given API key has read access to a commit that has already been fetched.
///
/// This is useful when you already have the commit entity and want to check
/// read access without a second database lookup.
///
/// # Errors
/// - `AuthzError::Forbidden` if the commit is private and the API key's org doesn't match
/// - `AuthzError::Db` for database errors
pub async fn check_commit_read_access_entity(
    db: &DB,
    api_key: &ApiKeyEntity,
    commit: &VmCommitEntity,
) -> Result<(), AuthzError> {
    if commit.is_public {
        return Ok(());
    }

    check_resource_ownership(db, api_key, commit.owner_id).await
}

/// Check if the given API key has access to the specified tag.
///
/// Returns the tag entity if access is granted.
///
/// # Errors
/// - `AuthzError::TagNotFound` if the tag does not exist
/// - `AuthzError::Forbidden` if the API key's org doesn't match the tag owner's org
/// - `AuthzError::Db` for database errors
pub async fn check_tag_access(
    db: &DB,
    api_key: &ApiKeyEntity,
    tag_id: Uuid,
) -> Result<CommitTagEntity, AuthzError> {
    let tag = db
        .commit_tags()
        .get_by_id(tag_id)
        .await?
        .ok_or(AuthzError::TagNotFound)?;

    check_resource_ownership(db, api_key, tag.owner_id).await?;

    Ok(tag)
}

/// Check if the given API key belongs to the same org as the resource owner.
///
/// # Errors
/// - `AuthzError::Forbidden` if the owner key doesn't exist or orgs don't match
/// - `AuthzError::Db` for database errors
pub async fn check_resource_ownership(
    db: &DB,
    api_key: &ApiKeyEntity,
    resource_owner_id: Uuid,
) -> Result<(), AuthzError> {
    let owner_key = db
        .keys()
        .get_by_id(resource_owner_id)
        .await?
        .ok_or(AuthzError::Forbidden)?;

    if owner_key.org_id() != api_key.org_id() {
        return Err(AuthzError::Forbidden);
    }

    Ok(())
}
