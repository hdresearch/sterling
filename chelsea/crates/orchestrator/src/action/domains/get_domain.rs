use thiserror::Error;
use uuid::Uuid;

use crate::db::{ApiKeyEntity, DB, DBError, DomainsRepository};

use super::DomainResponse;

/// Get a custom domain by ID.
#[derive(Debug, Clone)]
pub struct GetDomain {
    domain_id: Uuid,
    api_key: ApiKeyEntity,
    request_id: Option<String>,
}

impl GetDomain {
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
pub enum GetDomainError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
}

impl GetDomain {
    pub async fn call(self, db: &DB) -> Result<DomainResponse, GetDomainError> {
        // 1. Get the domain
        let domain = db
            .domains()
            .get_by_id(self.domain_id)
            .await?
            .ok_or(GetDomainError::NotFound)?;

        // 2. Check ownership (domain owner_id is a user_id, compare directly)
        if domain.owner_id() != self.api_key.id() {
            return Err(GetDomainError::Forbidden);
        }

        Ok(DomainResponse {
            domain_id: domain.domain_id(),
            vm_id: domain.vm_id(),
            domain: domain.domain().to_string(),
            created_at: domain.created_at(),
        })
    }
}

impl_error_response!(GetDomainError,
    GetDomainError::Db(_) => INTERNAL_SERVER_ERROR,
    GetDomainError::Forbidden => FORBIDDEN,
    GetDomainError::NotFound => NOT_FOUND,
);
