use dto_lib::orchestrator::commit_repository::CreateRepositoryResponse;
use thiserror::Error;

use crate::db::{ApiKeyEntity, CommitRepositoriesRepository, DBError};

#[derive(Debug, Clone)]
pub struct CreateRepository {
    pub name: String,
    pub description: Option<String>,
    pub api_key: ApiKeyEntity,
}

impl CreateRepository {
    pub fn new(name: String, description: Option<String>, api_key: ApiKeyEntity) -> Self {
        Self {
            name,
            description,
            api_key,
        }
    }

    /// Validate repository name: alphanumeric, hyphens, underscores, dots, slashes, 1-128 chars
    fn validate_name(name: &str) -> Result<(), CreateRepositoryError> {
        if name.is_empty() || name.len() > 128 {
            return Err(CreateRepositoryError::InvalidName(
                "Repository name must be 1-128 characters".to_string(),
            ));
        }

        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
        {
            return Err(CreateRepositoryError::InvalidName(
                "Repository name can only contain alphanumeric characters, hyphens, underscores, dots, and slashes"
                    .to_string(),
            ));
        }

        // Don't allow leading/trailing slashes or consecutive slashes
        if name.starts_with('/') || name.ends_with('/') || name.contains("//") {
            return Err(CreateRepositoryError::InvalidName(
                "Repository name cannot start/end with '/' or contain consecutive slashes"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CreateRepositoryError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("invalid repository name: {0}")]
    InvalidName(String),
    #[error("repository already exists")]
    AlreadyExists,
}

impl CreateRepository {
    pub async fn call(
        self,
        db: &crate::action::DB,
    ) -> Result<CreateRepositoryResponse, CreateRepositoryError> {
        Self::validate_name(&self.name)?;

        let repo = match db
            .commit_repositories()
            .insert(
                self.name.clone(),
                self.api_key.org_id(),
                self.api_key.id(),
                self.description,
            )
            .await
        {
            Ok(repo) => repo,
            Err(e) => {
                if let Some(db_err) = e.as_db_error() {
                    if db_err
                        .constraint()
                        .is_some_and(|c| c == "unique_repo_per_org")
                    {
                        return Err(CreateRepositoryError::AlreadyExists);
                    }
                }
                return Err(CreateRepositoryError::Db(e));
            }
        };

        tracing::info!(
            repo_id = %repo.id,
            name = %repo.name,
            org_id = %repo.org_id,
            "Created commit repository"
        );

        Ok(CreateRepositoryResponse {
            repo_id: repo.id,
            name: repo.name,
        })
    }
}

impl_error_response!(CreateRepositoryError,
    CreateRepositoryError::Db(_) => INTERNAL_SERVER_ERROR,
    CreateRepositoryError::InvalidName(_) => BAD_REQUEST,
    CreateRepositoryError::AlreadyExists => CONFLICT,
);
