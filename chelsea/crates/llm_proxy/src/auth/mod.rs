//! Authentication: virtual API key validation and budget enforcement.
//!
//! **Known limitation (TOCTOU on budget/credits):** The budget check runs at request
//! entry (in this middleware), but spend is recorded asynchronously after the response.
//! Under high concurrency, multiple in-flight requests can pass the budget check
//! simultaneously, leading to over-spend up to `max_concurrent_requests × max_single_request_cost`.
//!
//! This is acceptable for now because:
//! - LLM requests are slow (seconds), so the concurrency window per key is small
//! - The alternative (SELECT ... FOR UPDATE) would serialize all requests per key
//! - A post-facto reconciliation job can catch and flag over-spend
//!
//! If tighter enforcement is needed, consider an in-memory token-bucket per key or
//! Postgres advisory locks on the budget check.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rust_decimal::Decimal;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;
use crate::error::ProxyError;

/// Authenticated key info attached to request extensions after auth.
#[derive(Debug, Clone, Serialize)]
pub struct AuthenticatedKey {
    pub id: Uuid,
    pub key_prefix: String,
    pub name: Option<String>,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub spend: Decimal,
    pub credits: Decimal,
    pub max_budget: Option<Decimal>,
    pub models: Vec<String>,
}

/// Hash a raw API key to its stored form.
pub fn hash_api_key(raw_key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Generate a new virtual API key. Returns (raw_key, hash, prefix).
pub fn generate_api_key() -> (String, String, String) {
    use rand::Rng;
    let mut rng = rand::rng();
    let random_bytes: [u8; 32] = rng.random();
    let raw_key = format!("sk-vers-{}", hex::encode(random_bytes));
    let key_hash = hash_api_key(&raw_key);
    let prefix = format!("sk-vers-...{}", &raw_key[raw_key.len() - 6..]);
    (raw_key, key_hash, prefix)
}

/// Axum middleware that authenticates requests via Bearer token.
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    // Extract bearer token from Authorization header or x-api-key
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let api_key_header = request
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let raw_key = if let Some(bearer) = auth_header.strip_prefix("Bearer ") {
        bearer.trim()
    } else if !api_key_header.is_empty() {
        api_key_header.trim()
    } else {
        return ProxyError::MissingApiKey.into_response();
    };

    let key_hash = hash_api_key(raw_key);

    let key_row = match state.billing.get_api_key_by_hash(&key_hash).await {
        Ok(Some(row)) => row,
        Ok(None) => return ProxyError::InvalidApiKey.into_response(),
        Err(e) => return ProxyError::Database(e).into_response(),
    };

    // Key-level checks (revoked, expired) always apply regardless of billing mode.
    if let Some(ref reason) = key_row.deny_reason {
        match reason.as_str() {
            "key_revoked" => return ProxyError::KeyRevoked.into_response(),
            "key_expired" => return ProxyError::KeyExpired.into_response(),
            // Per-key budget cap still applies even with Stripe billing
            "budget_exceeded" => return ProxyError::BudgetExceeded.into_response(),
            _ => {}
        }
    }

    // Credit/balance gating: use Stripe balance cache if available, else local ledger.
    if let Some(ref cache) = state.balance_cache {
        // Stripe billing mode — check cached Stripe credit balance
        if let Some(team_id) = key_row.team_id {
            if let Some(cached) = cache.get(&team_id).await {
                if cached.effective_millicents() <= 0 {
                    return ProxyError::CreditsExhausted {
                        remaining: Decimal::ZERO,
                        spend: Decimal::ZERO,
                        credits: Decimal::ZERO,
                    }
                    .into_response();
                }
            }
            // Cache miss: allow through — better to over-serve than block a paying customer.
            // The next balance poll will populate the cache.
        }
    } else {
        // Local ledger mode — use the DB-computed deny_reason for credit checks
        if let Some(ref reason) = key_row.deny_reason {
            let err = match reason.as_str() {
                "no_credits" => ProxyError::NoCredits,
                "credits_exhausted" => ProxyError::CreditsExhausted {
                    remaining: (key_row.credits - key_row.spend).max(Decimal::ZERO),
                    spend: key_row.spend,
                    credits: key_row.credits,
                },
                other => ProxyError::Internal(format!("unknown deny reason: {other}")),
            };
            return err.into_response();
        }
    }

    let auth_key = AuthenticatedKey {
        id: key_row.id,
        key_prefix: key_row.key_prefix,
        name: key_row.name,
        user_id: key_row.user_id,
        team_id: key_row.team_id,
        spend: key_row.spend,
        credits: key_row.credits,
        max_budget: key_row.max_budget,
        models: key_row.models,
    };

    request.extensions_mut().insert(auth_key);
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let hash1 = hash_api_key("sk-vers-test-key-123");
        let hash2 = hash_api_key("sk-vers-test-key-123");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn different_keys_produce_different_hashes() {
        let hash1 = hash_api_key("sk-vers-key-aaa");
        let hash2 = hash_api_key("sk-vers-key-bbb");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn hash_is_hex_sha256() {
        let hash = hash_api_key("hello");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_api_key_format() {
        let (raw, hash, prefix) = generate_api_key();
        assert!(raw.starts_with("sk-vers-"));
        assert_eq!(hash, hash_api_key(&raw));
        assert!(prefix.starts_with("sk-vers-..."));
        assert_eq!(&prefix[11..], &raw[raw.len() - 6..]);
    }

    #[test]
    fn generated_keys_are_unique() {
        let (key1, _, _) = generate_api_key();
        let (key2, _, _) = generate_api_key();
        assert_ne!(key1, key2);
    }
}

// ApiKeyRow is defined in the billing crate: billing::db::ApiKeyRow
pub use billing::db::ApiKeyRow;
