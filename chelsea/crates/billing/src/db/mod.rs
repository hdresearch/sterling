//! Database layer for billing tables.
//!
//! Tables owned by this module:
//! - `llm_teams` — team-level credit pools (1:1 with orgs)
//! - `llm_api_keys` — virtual API keys for the LLM proxy
//! - `llm_credit_transactions` — append-only credit ledger
//! - `vers_landing.org_subscriptions` — subscription + Stripe customer mapping

mod credits;
mod keys;
mod pool;
mod subscriptions;
mod teams;

pub use pool::make_pool;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::error::DbError;

pub type PgPool =
    bb8::Pool<bb8_postgres::PostgresConnectionManager<tokio_postgres_rustls::MakeRustlsConnect>>;

// ─── Shared helpers ──────────────────────────────────────────────────────────

/// Convert Decimal to f64 for DOUBLE PRECISION columns.
pub fn to_f64(d: &Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

pub fn to_f64_opt(d: &Option<Decimal>) -> Option<f64> {
    d.as_ref().map(to_f64)
}

/// Read an f64 column as Decimal.
pub fn decimal_from_row(row: &tokio_postgres::Row, col: &str) -> Decimal {
    let val: f64 = row.get(col);
    Decimal::try_from(val).unwrap_or_default()
}

pub fn decimal_opt_from_row(row: &tokio_postgres::Row, col: &str) -> Option<Decimal> {
    let val: Option<f64> = row.get(col);
    val.and_then(|v| Decimal::try_from(v).ok())
}

// ─── Public types ────────────────────────────────────────────────────────────

/// The main billing database handle. Provides access to all billing table operations.
#[derive(Debug, Clone)]
pub struct BillingDb {
    pool: PgPool,
}

impl BillingDb {
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        Ok(Self {
            pool: make_pool(url, "billing").await?,
        })
    }

    pub async fn ping(&self) -> bool {
        match self.pool.get().await {
            Ok(conn) => conn.execute("SELECT 1", &[]).await.is_ok(),
            Err(_) => false,
        }
    }

    /// Run billing migrations (for tests — production uses dbmate).
    pub async fn migrate(&self) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        let sql = include_str!("../migrations/001_billing_tables.sql");
        conn.batch_execute(sql)
            .await
            .map_err(|e| DbError::Migration(e.to_string()))?;
        tracing::info!("billing database migrations applied");
        Ok(())
    }
}

// ─── Row types ───────────────────────────────────────────────────────────────

/// API key row returned from auth queries.
#[derive(Debug, Clone)]
pub struct ApiKeyRow {
    pub id: Uuid,
    pub key_prefix: String,
    pub name: Option<String>,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub spend: Decimal,
    pub credits: Decimal,
    pub max_budget: Option<Decimal>,
    pub models: Vec<String>,
    pub revoked: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub deny_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TeamRow {
    pub id: Uuid,
    pub org_id: Option<Uuid>,
    pub name: String,
    pub spend: Decimal,
    pub credits: Decimal,
    pub max_budget: Option<Decimal>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreditTransaction {
    pub id: Uuid,
    pub amount: Decimal,
    pub balance_after: Decimal,
    pub description: String,
    pub reference_id: Option<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Row from the Vers platform `api_keys` table (not LLM keys).
#[derive(Debug, Clone)]
pub struct VersApiKeyRow {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub label: String,
    pub iterations: i32,
    pub salt: String,
    pub hash: String,
}

/// Mapping from team to its Stripe customer, with auto-topup config.
#[derive(Debug, Clone)]
pub struct TeamStripeMapping {
    pub team_id: Uuid,
    pub org_id: Uuid,
    pub stripe_customer_id: String,
    pub auto_topup_enabled: bool,
    pub auto_topup_threshold_cents: i32,
    pub auto_topup_amount_cents: i32,
}

/// Subscription row from `vers_landing.org_subscriptions`.
#[derive(Debug, Clone)]
pub struct OrgSubscription {
    pub org_id: Uuid,
    pub tier: String,
    pub status: String,
    pub billing_provider: String,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub auto_topup_enabled: bool,
    pub auto_topup_threshold_cents: i32,
    pub auto_topup_amount_cents: i32,
}

/// Parameters for upserting an org subscription (used by webhook handler and DB layer).
pub struct UpsertSubscription<'a> {
    pub org_id: &'a str,
    pub tier: &'a str,
    pub status: &'a str,
    pub billing_provider: &'a str,
    pub customer_id: Option<&'a str>,
    pub subscription_id: Option<&'a str>,
    pub product_id: Option<&'a str>,
    pub price_id: Option<&'a str>,
    pub is_free_plan: bool,
}
