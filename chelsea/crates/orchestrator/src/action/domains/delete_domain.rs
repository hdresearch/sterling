use thiserror::Error;
use uuid::Uuid;

use crate::db::{ApiKeyEntity, DB, DBError, DomainsRepository};

/// Delete a custom domain.
#[derive(Debug, Clone)]
pub struct DeleteDomain {
    domain_id: Uuid,
    api_key: ApiKeyEntity,
    request_id: Option<String>,
}

impl DeleteDomain {
    pub fn new(domain_id: Uuid, api_key: ApiKeyEntity) -> Self {
        Self {
            domain_id,
            api_key,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum DeleteDomainError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
}

impl DeleteDomain {
    pub async fn call(self, db: &DB) -> Result<Uuid, DeleteDomainError> {
        // 1. Get the domain
        let domain = db
            .domains()
            .get_by_id(self.domain_id)
            .await?
            .ok_or(DeleteDomainError::NotFound)?;

        // 2. Check ownership (domain owner_id is a user_id, compare directly)
        if domain.owner_id() != self.api_key.id() {
            return Err(DeleteDomainError::Forbidden);
        }

        // 3. Delete the domain
        db.domains().delete(self.domain_id).await?;

        tracing::info!(
            domain_id = %self.domain_id,
            "Deleted domain"
        );

        Ok(self.domain_id)
    }
}

impl_error_response!(DeleteDomainError,
    DeleteDomainError::Db(_) => INTERNAL_SERVER_ERROR,
    DeleteDomainError::Forbidden => FORBIDDEN,
    DeleteDomainError::NotFound => NOT_FOUND,
);
