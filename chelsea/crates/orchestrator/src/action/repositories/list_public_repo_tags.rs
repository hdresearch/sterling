use dto_lib::orchestrator::commit_repository::{ListRepoTagsResponse, RepoTagInfo};
use thiserror::Error;

use crate::db::{CommitRepositoriesRepository, CommitTagsRepository, DBError};

#[derive(Debug, Clone)]
pub struct ListPublicRepoTags {
    pub org_name: String,
    pub repo_name: String,
}

impl ListPublicRepoTags {
    pub fn new(org_name: String, repo_name: String) -> Self {
        Self {
            org_name,
            repo_name,
        }
    }
}

#[derive(Debug, Error)]
pub enum ListPublicRepoTagsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    NotFound,
}

impl ListPublicRepoTags {
    pub async fn call(
        self,
        db: &crate::action::DB,
    ) -> Result<ListRepoTagsResponse, ListPublicRepoTagsError> {
        let repo = db
            .commit_repositories()
            .get_public_by_org_and_name(&self.org_name, &self.repo_name)
            .await?
            .ok_or(ListPublicRepoTagsError::NotFound)?;

        let tags = db.commit_tags().list_by_repo(repo.id).await?;

        let tag_infos = tags
            .into_iter()
            .map(|t| RepoTagInfo {
                tag_id: t.id,
                tag_name: t.tag_name.clone(),
                reference: format!("{}:{}", repo.name, t.tag_name),
                commit_id: t.commit_id,
                description: t.description,
                created_at: t.created_at,
                updated_at: t.updated_at,
            })
            .collect();

        Ok(ListRepoTagsResponse {
            repository: format!("{}/{}", self.org_name, repo.name),
            tags: tag_infos,
        })
    }
}

impl_error_response!(ListPublicRepoTagsError,
    ListPublicRepoTagsError::Db(_) => INTERNAL_SERVER_ERROR,
    ListPublicRepoTagsError::NotFound => NOT_FOUND,
);
