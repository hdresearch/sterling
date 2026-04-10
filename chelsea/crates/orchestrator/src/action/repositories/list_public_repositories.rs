use dto_lib::orchestrator::commit_repository::{
    ListPublicRepositoriesResponse, PublicRepositoryInfo,
};
use thiserror::Error;

use crate::db::{CommitRepositoriesRepository, DBError, OrgsRepository};

#[derive(Debug, Clone)]
pub struct ListPublicRepositories;

impl ListPublicRepositories {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Error)]
pub enum ListPublicRepositoriesError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
}

impl ListPublicRepositories {
    pub async fn call(
        self,
        db: &crate::action::DB,
    ) -> Result<ListPublicRepositoriesResponse, ListPublicRepositoriesError> {
        let repos = db.commit_repositories().list_public().await?;

        let mut result = Vec::with_capacity(repos.len());
        for repo in repos {
            let org_name = match db.orgs().get_by_id(repo.org_id).await? {
                Some(org) => org.name().to_string(),
                None => continue, // skip repos with missing orgs
            };
            let full_name = format!("{}/{}", org_name, repo.name);
            result.push(PublicRepositoryInfo {
                repo_id: repo.id,
                org_name,
                name: repo.name,
                full_name,
                description: repo.description,
                created_at: repo.created_at,
            });
        }

        Ok(ListPublicRepositoriesResponse {
            repositories: result,
        })
    }
}

impl_error_response!(ListPublicRepositoriesError,
    ListPublicRepositoriesError::Db(_) => INTERNAL_SERVER_ERROR,
);
