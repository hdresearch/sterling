use std::time::Instant;

use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::pg;

pub fn hash(salt: &str, iter: i32, given: &str) -> String {
    tracing::trace!(iter = iter, salt_len = salt.len(), "Hashing API key");
    let mut key = [0u8; 64];
    let iter = u32::try_from(iter).unwrap();
    pbkdf2_hmac::<Sha256>(
        given.as_bytes(),
        &hex::decode(salt).unwrap(),
        iter,
        &mut key,
    );
    hex::encode(&key)
}

#[tracing::instrument(skip(key), fields(otel.name = "api_key.verify"))]
pub async fn verify(id: &str, key: &str) -> bool {
    let total_start = Instant::now();

    let uuid = Uuid::parse_str(id);
    if uuid.is_err() {
        tracing::debug!(key_id = %id, "Invalid UUID format for API key ID");
        return false;
    }

    let uuid = uuid.unwrap();

    let db_start = Instant::now();
    let res = pg::get_api_key(&uuid).await;
    let db_elapsed = db_start.elapsed();
    tracing::info!(key_id = %uuid, elapsed_ms = %db_elapsed.as_millis(), "api_key db_lookup");

    match res {
        Ok(Some(api_key)) => {
            let hash_start = Instant::now();
            let given_hash = hash(api_key.salt.as_str(), api_key.iter, key);
            let hash_elapsed = hash_start.elapsed();
            tracing::info!(
                key_id = %uuid,
                iter = api_key.iter,
                elapsed_ms = %hash_elapsed.as_millis(),
                "api_key pbkdf2_hash"
            );

            // Use constant-time comparison to prevent timing attacks
            let is_valid = api_key.hash.as_bytes().ct_eq(given_hash.as_bytes()).into();

            let total_elapsed = total_start.elapsed();
            if is_valid {
                tracing::info!(key_id = %uuid, elapsed_ms = %total_elapsed.as_millis(), "api_key verify success");
            } else {
                tracing::warn!(key_id = %uuid, elapsed_ms = %total_elapsed.as_millis(), "api_key verify failed - hash mismatch");
            }

            is_valid
        }
        Ok(None) => {
            let total_elapsed = total_start.elapsed();
            tracing::warn!(key_id = %uuid, elapsed_ms = %total_elapsed.as_millis(), "api_key not found or inactive");
            false
        }
        Err(e) => {
            let total_elapsed = total_start.elapsed();
            tracing::error!(
                key_id = %uuid,
                elapsed_ms = %total_elapsed.as_millis(),
                error = ?e,
                "api_key db error"
            );
            false
        }
    }
}
