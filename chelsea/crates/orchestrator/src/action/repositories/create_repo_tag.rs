use dto_lib::orchestrator::commit_repository::CreateRepoTagResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    action::{AuthzError, check_commit_access},
    db::{ApiKeyEntity, CommitRepositoriesRepository, CommitTagsRepository, DBError},
};

#[derive(Debug, Clone)]
pub struct CreateRepoTag {
    pub repo_name: String,
    pub tag_name: String,
    pub commit_id: Uuid,
    pub description: Option<String>,
    pub api_key: ApiKeyEntity,
}

impl CreateRepoTag {
    pub fn new(
        repo_name: String,
        tag_name: String,
        commit_id: Uuid,
        description: Option<String>,
        api_key: ApiKeyEntity,
    ) -> Self {
        Self {
            repo_name,
            tag_name,
            commit_id,
            description,
            api_key,
        }
    }

    /// Validate tag name: alphanumeric, hyphens, underscores, dots, 1-128 chars
    fn validate_tag_name(name: &str) -> Result<(), CreateRepoTagError> {
        if name.is_empty() || name.len() > 128 {
            return Err(CreateRepoTagError::InvalidTagName(
                "Tag name must be 1-128 characters".to_string(),
            ));
        }

        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(CreateRepoTagError::InvalidTagName(
                "Tag name can only contain alphanumeric characters, hyphens, underscores, and dots"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CreateRepoTagError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    RepositoryNotFound,
    #[error("commit not found")]
    CommitNotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("tag already exists in repository")]
    TagAlreadyExists,
    #[error("invalid tag name: {0}")]
    InvalidTagName(String),
}

impl CreateRepoTag {
    pub async fn call(
        self,
        db: &crate::action::DB,
    ) -> Result<CreateRepoTagResponse, CreateRepoTagError> {
        // 1. Validate tag name
        Self::validate_tag_name(&self.tag_name)?;

        // 2. Look up the repository
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.repo_name)
            .await?
            .ok_or(CreateRepoTagError::RepositoryNotFound)?;

        // 3. Check that user has org-level access to the commit
        let _commit = check_commit_access(&db, &self.api_key, self.commit_id)
            .await
            .map_err(|e| match e {
                AuthzError::CommitNotFound => CreateRepoTagError::CommitNotFound,
                AuthzError::Forbidden => CreateRepoTagError::Forbidden,
                AuthzError::Db(db) => CreateRepoTagError::Db(db),
                _ => CreateRepoTagError::Forbidden,
            })?;

        // 4. Insert the tag scoped to the repository
        let tag = match db
            .commit_tags()
            .insert_with_repo(
                self.tag_name.clone(),
                self.commit_id,
                self.api_key.id(),
                self.api_key.org_id(),
                repo.id,
                self.description,
            )
            .await
        {
            Ok(tag) => tag,
            Err(e) => {
                if let Some(db_err) = e.as_db_error() {
                    if db_err
                        .constraint()
                        .is_some_and(|c| c == "unique_tag_per_repo")
                    {
                        return Err(CreateRepoTagError::TagAlreadyExists);
                    }
                }
                return Err(CreateRepoTagError::Db(e));
            }
        };

        let reference = format!("{}:{}", self.repo_name, tag.tag_name);

        tracing::info!(
            tag_id = %tag.id,
            reference = %reference,
            commit_id = %tag.commit_id,
            repo_id = %repo.id,
            "Created repo tag"
        );

        Ok(CreateRepoTagResponse {
            tag_id: tag.id,
            reference,
            commit_id: tag.commit_id,
        })
    }
}

impl_error_response!(CreateRepoTagError,
    CreateRepoTagError::Db(_) => INTERNAL_SERVER_ERROR,
    CreateRepoTagError::RepositoryNotFound => NOT_FOUND,
    CreateRepoTagError::CommitNotFound => NOT_FOUND,
    CreateRepoTagError::Forbidden => FORBIDDEN,
    CreateRepoTagError::TagAlreadyExists => CONFLICT,
    CreateRepoTagError::InvalidTagName(_) => BAD_REQUEST,
);
