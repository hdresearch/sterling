//! Key exchange: trade a valid Vers platform API key for an LLM proxy API key.
//!
//! `POST /v1/keys/exchange`
//!
//! The caller sends their Vers platform API key (the same token used for the
//! Vers API: a 36-char UUID concatenated with a 64-char hex secret).
//! We validate it against the `api_keys` table using PBKDF2, then find-or-create
//! an `llm_teams` row for their org and return a fresh `sk-vers-*` LLM API key.

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::AppState;
use crate::auth;
use crate::error::ProxyError;

pub fn routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/keys/exchange", post(exchange_key))
        .with_state(state)
}

// ─── Request / Response ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ExchangeRequest {
    /// The Vers platform API key (`<uuid><64-char-hex-secret>`, 100 chars total).
    vers_api_key: String,
    /// Optional friendly name for the LLM key.
    name: Option<String>,
}

#[derive(Serialize)]
struct ExchangeResponse {
    /// The new LLM proxy API key (`sk-vers-*`). Show once, not stored in plaintext.
    key: String,
    /// Truncated prefix for display.
    key_prefix: String,
    /// The LLM key ID.
    id: Uuid,
    /// The team this key belongs to.
    team_id: Uuid,
}

// ─── Vers API Key Parsing & Verification ─────────────────────────────────────

/// Parse a Vers API key into (api_key_id, secret).
/// Format: `<36-char-uuid><64-char-hex-secret>` = 100 chars total.
fn parse_vers_key(raw: &str) -> Result<(Uuid, &str), ProxyError> {
    let raw = raw.trim();
    if raw.len() != 100 {
        return Err(ProxyError::InvalidApiKey);
    }
    let id = Uuid::parse_str(&raw[..36]).map_err(|_| ProxyError::InvalidApiKey)?;
    let secret = &raw[36..];
    Ok((id, secret))
}

/// Hash a raw key with PBKDF2-HMAC-SHA256, matching the Vers platform's scheme.
fn pbkdf2_hash(salt_hex: &str, iterations: u32, raw_key: &str) -> Result<String, ProxyError> {
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha256;

    let salt = hex::decode(salt_hex)
        .map_err(|e| ProxyError::Internal(format!("bad salt hex in api_keys row: {e}")))?;

    let mut derived = [0u8; 64];
    pbkdf2_hmac::<Sha256>(raw_key.as_bytes(), &salt, iterations, &mut derived);
    Ok(hex::encode(derived))
}

// ─── Handler ─────────────────────────────────────────────────────────────────

async fn exchange_key(
    State(state): State<Arc<AppState>>,
    axum::Json(req): axum::Json<ExchangeRequest>,
) -> Result<Response, ProxyError> {
    // 1. Parse the Vers API key.
    let (api_key_id, secret) = parse_vers_key(&req.vers_api_key)?;

    // 2. Look up the key in the billing DB.
    let vers_key = state
        .billing
        .get_vers_api_key(api_key_id)
        .await?
        .ok_or(ProxyError::InvalidApiKey)?;

    // 3. Verify with PBKDF2.
    let computed_hash = pbkdf2_hash(&vers_key.salt, vers_key.iterations as u32, secret)?;

    let is_valid: bool = computed_hash
        .as_bytes()
        .ct_eq(vers_key.hash.as_bytes())
        .into();

    if !is_valid {
        return Err(ProxyError::InvalidApiKey);
    }

    // 4. Find or create an llm_teams row for this org.
    let team_id = state.billing.find_or_create_team(vers_key.org_id).await?;

    // 5. Generate a new LLM proxy API key.
    let (raw_key, key_hash, key_prefix) = auth::generate_api_key();
    let id = Uuid::new_v4();
    let name = req
        .name
        .unwrap_or_else(|| format!("exchanged-{}", &vers_key.label));

    state
        .billing
        .create_api_key(
            id,
            &key_hash,
            &key_prefix,
            Some(&name),
            Some(vers_key.user_id),
            Some(team_id),
            None, // no budget limit
            &[],  // all models
            None, // no expiry
        )
        .await?;

    // 6. Return the raw key (shown once).
    Ok((
        StatusCode::OK,
        axum::Json(ExchangeResponse {
            key: raw_key,
            key_prefix,
            id,
            team_id,
        }),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_vers_key() {
        let uuid = "1795abdf-14c1-40e0-9a5d-778b09cf8cb3";
        let secret = "bfa85827e1f1ebab3078c3d3249a72647aef57451bd5feac7b727dcb5842590c";
        let raw = format!("{uuid}{secret}");
        let (id, s) = parse_vers_key(&raw).unwrap();
        assert_eq!(id.to_string(), uuid);
        assert_eq!(s, secret);
    }

    #[test]
    fn parse_short_key_rejected() {
        assert!(parse_vers_key("too-short").is_err());
    }

    #[test]
    fn parse_bad_uuid_rejected() {
        let bad = format!("{}{}", "x".repeat(36), "a".repeat(64));
        assert!(parse_vers_key(&bad).is_err());
    }
}
