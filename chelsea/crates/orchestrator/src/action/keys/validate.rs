use thiserror::Error;

use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use uuid::Uuid;

use crate::db::{ApiKeyEntity, ApiKeysRepository, DB, DBError};

/// Validate a plaintext API key and return the matching entity if valid.
#[derive(Clone)]
pub struct ValidateApiKey {
    pub token: String,
}

impl ValidateApiKey {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}
use std::str::FromStr;

#[derive(Debug, Error)]
pub enum ValidateApiKeyError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("invalid api key")]
    Invalid,
}

impl ValidateApiKey {
    pub fn parse_key(key: &str) -> Option<(Uuid, &str)> {
        if key.len() == 100 {
            Some((Uuid::from_str(&key[..36]).ok()?, &key[36..100]))
        } else {
            None
        }
    }
    pub fn hash(salt: &str, iter: i32, given: &str) -> anyhow::Result<String> {
        let mut key = [0u8; 64];
        let iter = u32::try_from(iter)?;
        pbkdf2_hmac::<Sha256>(given.as_bytes(), &hex::decode(salt)?, iter, &mut key);
        Ok(hex::encode(&key))
    }
}

impl ValidateApiKey {
    #[tracing::instrument(skip_all, fields(action = "keys.validate"))]
    pub async fn call(self, db: &DB) -> Result<ApiKeyEntity, ValidateApiKeyError> {
        let Some((id, key)) = Self::parse_key(&self.token) else {
            return Err(ValidateApiKeyError::Invalid);
        };

        let Some(api_key) = db.keys().get_by_id(id).await? else {
            return Err(ValidateApiKeyError::Invalid);
        };

        let given_hash = Self::hash(&api_key.salt(), api_key.iter(), key)
            .map_err(|_| ValidateApiKeyError::Invalid)?;

        if api_key.hash() == &given_hash {
            Ok(api_key)
        } else {
            Err(ValidateApiKeyError::Invalid)
        }
    }
}
