//! Billing infrastructure: Stripe integration, credit management, and subscription state.
//!
//! This crate is the single owner of all billing concerns:
//! - Stripe API client (meter events, credit balance, customers, webhooks)
//! - Database operations for billing tables (llm_teams, llm_api_keys, org_subscriptions)
//! - Balance cache and meter event batching
//!
//! Consumed by `llm_proxy` (auth gating, spend tracking) and `orchestrator` (usage metering).

pub mod db;
pub mod error;
pub mod http;
pub mod stripe;
