use thiserror::Error;

use crate::{
    action::{AuthzError, check_resource_ownership},
    db::{ApiKeyEntity, CommitRepositoriesRepository, DBError},
};

#[derive(Debug, Clone)]
pub struct SetRepositoryVisibility {
    pub repo_name: String,
    pub is_public: bool,
    pub api_key: ApiKeyEntity,
}

impl SetRepositoryVisibility {
    pub fn new(repo_name: String, is_public: bool, api_key: ApiKeyEntity) -> Self {
        Self {
            repo_name,
            is_public,
            api_key,
        }
    }
}

#[derive(Debug, Error)]
pub enum SetRepositoryVisibilityError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("repository not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
}

impl SetRepositoryVisibility {
    pub async fn call(self, db: &crate::action::DB) -> Result<(), SetRepositoryVisibilityError> {
        let repo = db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &self.repo_name)
            .await?
            .ok_or(SetRepositoryVisibilityError::NotFound)?;

        check_resource_ownership(&db, &self.api_key, repo.owner_id)
            .await
            .map_err(|e| match e {
                AuthzError::Forbidden => SetRepositoryVisibilityError::Forbidden,
                AuthzError::Db(db) => SetRepositoryVisibilityError::Db(db),
                _ => SetRepositoryVisibilityError::Forbidden,
            })?;

        db.commit_repositories()
            .set_public(repo.id, self.is_public)
            .await?;

        Ok(())
    }
}

impl_error_response!(SetRepositoryVisibilityError,
    SetRepositoryVisibilityError::Db(_) => INTERNAL_SERVER_ERROR,
    SetRepositoryVisibilityError::NotFound => NOT_FOUND,
    SetRepositoryVisibilityError::Forbidden => FORBIDDEN,
);
