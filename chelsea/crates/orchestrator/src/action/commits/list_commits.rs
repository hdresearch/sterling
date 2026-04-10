use thiserror::Error;

use crate::db::{ApiKeyEntity, DB, DBError, VMCommitsRepository, VmCommitEntity};
use dto_lib::chelsea_server2::commits::{CommitInfo, ListCommitsResponse};

pub const DEFAULT_PAGE_SIZE: i64 = 50;
pub const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone)]
pub struct ListCommits {
    key: ApiKeyEntity,
    limit: i64,
    offset: i64,
    /// If true, list public commits from all users instead of the caller's own commits.
    public_only: bool,
}

impl ListCommits {
    pub fn new(key: ApiKeyEntity, limit: Option<i64>, offset: Option<i64>) -> Self {
        let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE).max(1);
        let offset = offset.unwrap_or(0).max(0);
        Self {
            key,
            limit,
            offset,
            public_only: false,
        }
    }

    pub fn public(key: ApiKeyEntity, limit: Option<i64>, offset: Option<i64>) -> Self {
        let mut s = Self::new(key, limit, offset);
        s.public_only = true;
        s
    }
}

#[derive(Debug, Error)]
pub enum ListCommitsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("forbidden")]
    Forbidden,
}

impl From<VmCommitEntity> for CommitInfo {
    fn from(entity: VmCommitEntity) -> Self {
        Self {
            commit_id: entity.id.to_string(),
            parent_vm_id: entity.parent_vm_id.map(|id| id.to_string()),
            grandparent_commit_id: entity.grandparent_commit_id.map(|id| id.to_string()),
            owner_id: entity.owner_id.to_string(),
            name: entity.name,
            description: entity.description,
            created_at: entity.created_at.to_rfc3339(),
            is_public: entity.is_public,
        }
    }
}

impl ListCommits {
    pub async fn call(self, db: &DB) -> Result<ListCommitsResponse, ListCommitsError> {
        let commits_repo = db.commits();

        let (commits, total) = if self.public_only {
            let (commits_result, total_result) = tokio::join!(
                commits_repo.list_public(self.limit, self.offset),
                commits_repo.count_public(),
            );
            (commits_result?, total_result?)
        } else {
            let owner_id = self.key.id();
            let (commits_result, total_result) = tokio::join!(
                commits_repo.list_by_owner(owner_id, self.limit, self.offset),
                commits_repo.count_by_owner(owner_id),
            );
            (commits_result?, total_result?)
        };

        Ok(ListCommitsResponse {
            commits: commits.into_iter().map(CommitInfo::from).collect(),
            total,
            limit: self.limit,
            offset: self.offset,
        })
    }
}

impl_error_response!(ListCommitsError,
    ListCommitsError::Db(_) => INTERNAL_SERVER_ERROR,
    ListCommitsError::Forbidden => FORBIDDEN,
);
