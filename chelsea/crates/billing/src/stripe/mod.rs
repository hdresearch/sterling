//! Stripe billing integration.
//!
//! - [`client::StripeClient`] — HTTP client for Stripe's APIs
//! - [`meter::MeterEventSender`] — Batched, async meter event delivery
//! - [`balance::BalanceCache`] — Periodic credit balance polling with in-memory cache
//! - [`webhook`] — Webhook event processing (subscription lifecycle, credit grants)
//! - [`auto_topup`] — Periodic balance check and invoice creation

pub mod auto_topup;
pub mod balance;
pub mod client;
pub mod meter;
pub mod usage;
pub mod webhook;

#[cfg(test)]
mod auto_topup_tests;
#[cfg(test)]
mod balance_tests;
#[cfg(test)]
mod client_tests;
#[cfg(test)]
pub(crate) mod test_helpers;
#[cfg(test)]
mod webhook_tests;
