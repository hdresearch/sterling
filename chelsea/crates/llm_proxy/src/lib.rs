//! LLM Proxy — passthrough proxy with virtual API keys, spend tracking, and request logging.
//!
//! ## Design Decisions
//!
//! **Decimal for money:** Spend/credits use `rust_decimal::Decimal` in Rust and
//! `NUMERIC(20,8)` in Postgres to avoid IEEE 754 floating-point drift in billing.
//!
//! **Standalone config (not VersConfig):** This crate uses its own `AppConfig` loaded from
//! TOML + env var overrides, rather than the shared `VersConfig` system. This is intentional —
//! llm_proxy is deployed as a standalone ECS Fargate service, separate from the Chelsea
//! node fleet, and does not share the same INI-based config pipeline.

pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod providers;
pub mod request_id;
pub mod routing;
pub mod spend;
pub mod stripe;
pub mod types;

use crate::api::spend_tracking::CustomerIdCache;
use crate::config::AppConfig;
use crate::db::LogDb;
use crate::routing::ModelRouter;
use billing::db::BillingDb;
use billing::stripe::balance::BalanceCache;
use billing::stripe::meter::MeterEventSender;

/// Shared application state available to all handlers.
pub struct AppState {
    /// Billing DB (main DB): keys, teams, credits
    pub billing: BillingDb,
    /// Log DB (separate): spend_logs, request_logs
    pub logs: LogDb,
    pub config: AppConfig,
    pub router: ModelRouter,
    pub http_client: reqwest::Client,
    /// Stripe meter event sender (None if Stripe not configured).
    pub meter: Option<MeterEventSender>,
    /// Stripe credit balance cache (None if Stripe not configured).
    pub balance_cache: Option<BalanceCache>,
    /// Cached team_id → stripe_customer_id mappings.
    pub customer_id_cache: CustomerIdCache,
}
