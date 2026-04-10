use dto_lib::orchestrator::commit_repository::{ListRepoTagsResponse, RepoTagInfo};
use thiserror::Error;

use crate::db::{ApiKeyEntity, CommitRepositoriesRepository, CommitTagsRepository, DBError};

#[derive(Debug, Clone)]
pub struct ListRepoTags {
    pub repo_name: String,
    pub api_key: ApiKeyEntity,
}

impl ListRepoTags {
    pub fn new(repo_name: String, api_key: ApiKeyEntity) -> Self {
        Self { repo_name, api_key }
    }
}

#[derive(Debug, Error)]
pub enum ListRepoTagsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    RepositoryNotFound,
}

impl ListRepoTags {
    pub async fn call(
        self,
        db: &crate::action::DB,
    ) -> Result<ListRepoTagsResponse, ListRepoTagsError> {
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.repo_name)
            .await?
            .ok_or(ListRepoTagsError::RepositoryNotFound)?;

        let tags = db.commit_tags().list_by_repo(repo.id).await?;

        let tag_infos = tags
            .into_iter()
            .map(|tag| RepoTagInfo {
                tag_id: tag.id,
                tag_name: tag.tag_name.clone(),
                reference: format!("{}:{}", self.repo_name, tag.tag_name),
                commit_id: tag.commit_id,
                description: tag.description,
                created_at: tag.created_at,
                updated_at: tag.updated_at,
            })
            .collect();

        Ok(ListRepoTagsResponse {
            repository: self.repo_name,
            tags: tag_infos,
        })
    }
}

impl_error_response!(ListRepoTagsError,
    ListRepoTagsError::Db(_) => INTERNAL_SERVER_ERROR,
    ListRepoTagsError::RepositoryNotFound => NOT_FOUND,
);
