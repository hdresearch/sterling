use thiserror::Error;
use uuid::Uuid;

use crate::action::{AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, DB, DBError, DomainsRepository};

use super::DomainResponse;

/// List custom domains, optionally filtered by VM.
#[derive(Debug, Clone)]
pub struct ListDomains {
    vm_id: Option<Uuid>,
    api_key: ApiKeyEntity,
    request_id: Option<String>,
}

impl ListDomains {
    pub fn new(vm_id: Option<Uuid>, api_key: ApiKeyEntity) -> Self {
        Self {
            vm_id,
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
pub enum ListDomainsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("forbidden")]
    Forbidden,
    #[error("vm not found")]
    VmNotFound,
}

impl ListDomains {
    pub async fn call(self, db: &DB) -> Result<Vec<DomainResponse>, ListDomainsError> {
        let entities = if let Some(vm_id) = self.vm_id {
            // If vm_id is provided, check access then list domains for that VM
            check_vm_access(db, &self.api_key, vm_id)
                .await
                .map_err(|e| match e {
                    AuthzError::VmNotFound => ListDomainsError::VmNotFound,
                    AuthzError::Forbidden => ListDomainsError::Forbidden,
                    AuthzError::Db(db) => ListDomainsError::Db(db),
                    AuthzError::CommitNotFound => ListDomainsError::Forbidden,
                    AuthzError::TagNotFound => ListDomainsError::Forbidden,
                })?;

            db.domains().list_by_vm(vm_id).await?
        } else {
            // Otherwise, list all domains owned by the API key's user
            db.domains().list_by_owner(self.api_key.id()).await?
        };

        let domains = entities
            .into_iter()
            .map(|e| DomainResponse {
                domain_id: e.domain_id(),
                vm_id: e.vm_id(),
                domain: e.domain().to_string(),
                created_at: e.created_at(),
            })
            .collect();

        Ok(domains)
    }
}

impl_error_response!(ListDomainsError,
    ListDomainsError::Db(_) => INTERNAL_SERVER_ERROR,
    ListDomainsError::Forbidden => FORBIDDEN,
    ListDomainsError::VmNotFound => NOT_FOUND,
);
