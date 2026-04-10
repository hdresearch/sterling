//! Admin API endpoints for key management and spend queries.
//! Protected by the master admin API key from config.

use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use chrono::{DateTime, Utc};
use http::StatusCode;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth;
use crate::error::{DbError, ProxyError};

pub fn routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/keys", post(create_key))
        .route("/admin/keys", get(list_keys))
        .route("/admin/keys/{id}", delete(revoke_key))
        .route("/admin/keys/{id}/budget", post(update_key_budget))
        .route("/admin/keys/{id}/credits", post(add_credits))
        .route("/admin/keys/{id}/credits/history", get(credit_history))
        .route("/admin/spend", get(query_spend))
        .route("/admin/spend/models", get(query_spend_by_model))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            admin_auth_middleware,
        ))
}

async fn admin_auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let admin_key = match &state.config.server.admin_api_key {
        Some(key) => key.clone(),
        None => return ProxyError::AdminNotEnabled.into_response(),
    };

    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let provided = auth_header.strip_prefix("Bearer ").unwrap_or("");

    let is_valid: bool =
        subtle::ConstantTimeEq::ct_eq(provided.as_bytes(), admin_key.as_bytes()).into();

    if !is_valid {
        return ProxyError::InvalidAdminKey.into_response();
    }

    next.run(request).await
}

// ─── Key Management ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateKeyRequest {
    name: Option<String>,
    user_id: Option<Uuid>,
    team_id: Option<Uuid>,
    max_budget: Option<Decimal>,
    #[serde(default)]
    models: Vec<String>,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct CreateKeyResponse {
    key: String,
    key_prefix: String,
    id: Uuid,
}

async fn create_key(
    State(state): State<Arc<AppState>>,
    axum::Json(req): axum::Json<CreateKeyRequest>,
) -> Result<Response, ProxyError> {
    let (raw_key, key_hash, key_prefix) = auth::generate_api_key();
    let id = Uuid::new_v4();

    state
        .billing
        .create_api_key(
            id,
            &key_hash,
            &key_prefix,
            req.name.as_deref(),
            req.user_id,
            req.team_id,
            req.max_budget,
            &req.models,
            req.expires_at,
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        axum::Json(CreateKeyResponse {
            key: raw_key,
            key_prefix,
            id,
        }),
    )
        .into_response())
}

#[derive(Deserialize)]
struct ListKeysQuery {
    team_id: Option<Uuid>,
}

async fn list_keys(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListKeysQuery>,
) -> Result<Response, ProxyError> {
    let keys = state.billing.list_api_keys(query.team_id).await?;
    let keys: Vec<serde_json::Value> = keys
        .into_iter()
        .map(|k| {
            serde_json::json!({
                "id": k.id, "key_prefix": k.key_prefix, "name": k.name,
                "team_id": k.team_id,
                "spend": k.spend, "credits": k.credits,
                "remaining": (k.credits - k.spend).max(Decimal::ZERO),
                "max_budget": k.max_budget,
                "models": k.models, "revoked": k.revoked, "expires_at": k.expires_at,
            })
        })
        .collect();
    Ok((StatusCode::OK, axum::Json(serde_json::json!(keys))).into_response())
}

#[derive(Deserialize)]
struct UpdateKeyBudgetRequest {
    max_budget: Option<Decimal>,
    #[serde(default)]
    reset_spend: bool,
}

async fn update_key_budget(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    axum::Json(req): axum::Json<UpdateKeyBudgetRequest>,
) -> Result<Response, ProxyError> {
    let updated = state
        .billing
        .update_api_key_budget(id, req.max_budget, req.reset_spend)
        .await?;
    if !updated {
        return Err(ProxyError::NotFound { entity: "key" });
    }
    let msg = if req.reset_spend {
        "budget updated and spend reset"
    } else {
        "budget updated"
    };
    Ok((
        StatusCode::OK,
        axum::Json(serde_json::json!({"message": msg})),
    )
        .into_response())
}

async fn revoke_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Response, ProxyError> {
    let revoked = state.billing.revoke_api_key(id).await?;
    if !revoked {
        return Err(ProxyError::NotFound { entity: "key" });
    }
    Ok((
        StatusCode::OK,
        axum::Json(serde_json::json!({"revoked": true})),
    )
        .into_response())
}

// ─── Credits ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AddCreditsRequest {
    amount: Decimal,
    description: Option<String>,
    reference_id: Option<String>,
}

async fn add_credits(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    axum::Json(req): axum::Json<AddCreditsRequest>,
) -> Result<Response, ProxyError> {
    let description = req.description.as_deref().unwrap_or("admin top-up");

    // Look up the key's team so we add credits to the shared team pool.
    let key = state
        .billing
        .get_api_key_by_id(id)
        .await?
        .ok_or(ProxyError::NotFound { entity: "api key" })?;

    let team_id = key.team_id.ok_or(ProxyError::BadRequest {
        reason: "key has no team — cannot add credits".into(),
    })?;

    let new_balance = state
        .billing
        .add_team_credits(
            team_id,
            req.amount,
            description,
            req.reference_id.as_deref(),
            "admin",
        )
        .await
        .map_err(|e| match e {
            DbError::InvalidCreditAmount => ProxyError::BadRequest {
                reason: "credit amount must be positive".into(),
            },
            other => ProxyError::Database(other),
        })?;

    Ok((
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "credits": new_balance,
            "added": req.amount,
            "team_id": team_id,
        })),
    )
        .into_response())
}

async fn credit_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Response, ProxyError> {
    let history = state.billing.get_credit_history(id).await?;
    Ok((StatusCode::OK, axum::Json(history)).into_response())
}

// ─── Spend Queries ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SpendQuery {
    api_key_id: Option<Uuid>,
    since: Option<DateTime<Utc>>,
}

async fn query_spend(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SpendQuery>,
) -> Result<Response, ProxyError> {
    let key_id = query.api_key_id.ok_or(ProxyError::MissingField {
        field: "api_key_id",
    })?;
    let summary = state.logs.get_spend_by_key(key_id, query.since).await?;
    Ok((StatusCode::OK, axum::Json(summary)).into_response())
}

#[derive(Deserialize)]
struct SpendByModelQuery {
    since: Option<DateTime<Utc>>,
}

async fn query_spend_by_model(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SpendByModelQuery>,
) -> Result<Response, ProxyError> {
    let models = state.logs.get_spend_by_model(query.since).await?;
    Ok((StatusCode::OK, axum::Json(models)).into_response())
}
