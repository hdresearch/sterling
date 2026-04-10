//! Database layer.
//!
//! Billing operations (keys, teams, credits, subscriptions) are provided by the
//! `billing` crate. This module re-exports them and adds the log DB (spend_logs,
//! request_logs) which is specific to the LLM proxy.

mod logs;

// Re-export all billing DB types from the billing crate.
pub use billing::db::{
    ApiKeyRow, BillingDb, CreditTransaction, OrgSubscription, TeamRow, TeamStripeMapping,
    VersApiKeyRow,
};
pub use billing::error::DbError;

pub use logs::LogDb;

// Re-export shared types that logs.rs needs.
pub(crate) use billing::db::{decimal_from_row, to_f64};

// ─── Types used only by LogDb ────────────────────────────────────────────────

use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, serde::Serialize)]
pub struct SpendSummary {
    pub total_spend: Decimal,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub request_count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ModelSpend {
    pub model: String,
    pub total_spend: Decimal,
    pub total_tokens: i64,
    pub request_count: i64,
}

#[derive(Debug)]
pub struct RequestLogRow {
    pub id: Uuid,
    pub api_key_id: Uuid,
    pub team_id: Option<Uuid>,
    pub model: String,
    pub request_body: serde_json::Value,
    pub response_body: serde_json::Value,
    pub stop_reason: Option<String>,
    pub error_message: Option<String>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
}
