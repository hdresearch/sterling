use dto_lib::orchestrator::commit_repository::{ListRepositoriesResponse, RepositoryInfo};
use thiserror::Error;

use crate::db::{ApiKeyEntity, CommitRepositoriesRepository, DBError};

#[derive(Debug, Clone)]
pub struct ListRepositories {
    pub api_key: ApiKeyEntity,
}

impl ListRepositories {
    pub fn new(api_key: ApiKeyEntity) -> Self {
        Self { api_key }
    }
}

#[derive(Debug, Error)]
pub enum ListRepositoriesError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
}

impl ListRepositories {
    pub async fn call(
        self,
        db: &crate::action::DB,
    ) -> Result<ListRepositoriesResponse, ListRepositoriesError> {
        let repos = db
            .commit_repositories()
            .list_by_org(self.api_key.org_id())
            .await?;

        let repositories = repos
            .into_iter()
            .map(|r| RepositoryInfo {
                repo_id: r.id,
                name: r.name,
                description: r.description,
                is_public: r.is_public,
                created_at: r.created_at,
            })
            .collect();

        Ok(ListRepositoriesResponse { repositories })
    }
}

impl_error_response!(ListRepositoriesError,
    ListRepositoriesError::Db(_) => INTERNAL_SERVER_ERROR,
);
