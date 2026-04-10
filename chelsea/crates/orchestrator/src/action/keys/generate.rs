use chrono::Utc;
use rand::RngCore;
use thiserror::Error;
use uuid::Uuid;

use crate::db::{ApiKeysRepository, DB, DBError};

use super::ValidateApiKey;

pub struct GenerateApiKey {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub label: String,
}

pub struct GeneratedApiKey {
    /// The 100-char plaintext key: `{api_key_id}{raw_key_hex}`
    pub api_key: String,
    pub api_key_id: Uuid,
}

#[derive(Debug, Error)]
pub enum GenerateApiKeyError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("hash error: {0}")]
    HashError(anyhow::Error),
}

impl GenerateApiKey {
    pub async fn call(self, db: &DB) -> Result<GeneratedApiKey, GenerateApiKeyError> {
        // Generate random bytes in a block so that ThreadRng is dropped before the first .await.
        let (salt_hex, raw_key_hex, hash) = {
            let mut rng = rand::rng();

            let mut salt_bytes = [0u8; 16];
            rng.fill_bytes(&mut salt_bytes);
            let salt_hex = hex::encode(salt_bytes);

            let mut raw_key_bytes = [0u8; 32];
            rng.fill_bytes(&mut raw_key_bytes);
            let raw_key_hex = hex::encode(raw_key_bytes);

            let hash = ValidateApiKey::hash(&salt_hex, 100, &raw_key_hex)
                .map_err(GenerateApiKeyError::HashError)?;

            (salt_hex, raw_key_hex, hash)
        };

        let inserted = db
            .keys()
            .insert(
                self.user_id,
                self.org_id,
                &self.label,
                "PBKDF2",
                100,
                &salt_hex,
                &hash,
                Utc::now(),
                None,
            )
            .await?;

        Ok(GeneratedApiKey {
            api_key: format!("{}{}", inserted.id(), raw_key_hex),
            api_key_id: inserted.id(),
        })
    }
}
