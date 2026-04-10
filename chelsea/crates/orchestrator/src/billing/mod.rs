use thiserror::Error;
use uuid::Uuid;

use crate::db::{ApiKeysRepository, DB, DBError};

/// Errors that can arise while mapping API keys to billing identities.
#[derive(Debug, Error)]
pub enum BillingLookupError {
    #[error("api key '{0}' not found")]
    ApiKeyNotFound(Uuid),
    #[error("database error: {0}")]
    Db(#[from] DBError),
}

/// Fetch the Chelsea user ID associated with a given API key.
pub async fn user_id_for_api_key(db: &DB, api_key_id: Uuid) -> Result<Uuid, BillingLookupError> {
    let api_key = db
        .keys()
        .get_by_id(api_key_id)
        .await?
        .ok_or(BillingLookupError::ApiKeyNotFound(api_key_id))?;

    Ok(api_key.user_id())
}
