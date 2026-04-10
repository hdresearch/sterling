//! Credit balance cache backed by periodic Stripe API polling.
//!
//! The cache maps `team_id → available_cents`. Auth middleware reads from the
//! cache for fast gating; the background task refreshes balances every N seconds.
//!
//! On cache miss (new team, first request), the auth middleware falls back to
//! allowing the request through — better to slightly over-serve than to block
//! a paying customer because the cache hasn't warmed yet.
//!
//! Auto-topup is handled separately by the orchestrator, not here.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::client::StripeClient;
use crate::db::BillingDb;

/// Per-team cached balance info.
#[derive(Debug, Clone)]
pub struct CachedBalance {
    /// Available credit balance from Stripe, in cents.
    pub available_cents: i64,
    /// Spend accumulated locally since the last Stripe poll.
    /// Subtracted from available_cents for real-time gating.
    pub pending_spend_millicents: i64,
}

impl CachedBalance {
    /// Effective balance in millicents, accounting for pending local spend.
    pub fn effective_millicents(&self) -> i64 {
        (self.available_cents * 1000) - self.pending_spend_millicents
    }
}

/// Thread-safe balance cache.
#[derive(Clone)]
pub struct BalanceCache {
    inner: Arc<RwLock<HashMap<Uuid, CachedBalance>>>,
}

impl BalanceCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the cached balance for a team. Returns `None` on cache miss.
    pub async fn get(&self, team_id: &Uuid) -> Option<CachedBalance> {
        self.inner.read().await.get(team_id).cloned()
    }

    /// Record local spend against a team's cached balance.
    /// Called after each request to keep the gate accurate between Stripe polls.
    pub async fn record_spend(&self, team_id: &Uuid, spend_millicents: i64) {
        let mut map = self.inner.write().await;
        if let Some(entry) = map.get_mut(team_id) {
            entry.pending_spend_millicents += spend_millicents;
        }
    }

    /// Update a team's balance from a fresh Stripe poll.
    /// Resets pending spend since Stripe now reflects all metered usage.
    pub(crate) async fn update(&self, team_id: Uuid, available_cents: i64) {
        let mut map = self.inner.write().await;
        map.insert(
            team_id,
            CachedBalance {
                available_cents,
                pending_spend_millicents: 0,
            },
        );
    }

    /// Remove teams that no longer have Stripe billing.
    pub(crate) async fn retain_teams(&self, active_team_ids: &[Uuid]) {
        let mut map = self.inner.write().await;
        map.retain(|k, _| active_team_ids.contains(k));
    }
}

/// Spawn the background balance refresh task.
pub fn spawn_balance_poller(
    client: StripeClient,
    billing_db: BillingDb,
    cache: BalanceCache,
    poll_interval: Duration,
) {
    tokio::spawn(async move {
        info!(
            poll_interval_secs = poll_interval.as_secs(),
            "stripe balance poller started"
        );

        // Initial poll immediately
        poll_all_balances(&client, &billing_db, &cache).await;

        let mut interval = tokio::time::interval(poll_interval);
        interval.tick().await; // skip the first immediate tick
        loop {
            interval.tick().await;
            poll_all_balances(&client, &billing_db, &cache).await;
        }
    });
}

async fn poll_all_balances(client: &StripeClient, billing_db: &BillingDb, cache: &BalanceCache) {
    let mappings = match billing_db.get_stripe_team_mappings().await {
        Ok(m) => m,
        Err(e) => {
            error!(error = %e, "failed to fetch team → Stripe customer mappings");
            return;
        }
    };

    if mappings.is_empty() {
        debug!("no teams with Stripe billing, skipping balance poll");
        return;
    }

    let team_ids: Vec<Uuid> = mappings.iter().map(|m| m.team_id).collect();
    cache.retain_teams(&team_ids).await;

    let mut success = 0u32;
    let mut errors = 0u32;

    for mapping in &mappings {
        match client.get_credit_balance(&mapping.stripe_customer_id).await {
            Ok(cents) => {
                cache.update(mapping.team_id, cents).await;
                debug!(
                    team_id = %mapping.team_id,
                    stripe_customer = %mapping.stripe_customer_id,
                    available_cents = cents,
                    "balance updated"
                );
                success += 1;
            }
            Err(e) => {
                warn!(
                    team_id = %mapping.team_id,
                    stripe_customer = %mapping.stripe_customer_id,
                    error = %e,
                    "failed to poll Stripe balance"
                );
                errors += 1;
            }
        }
    }

    info!(
        total = mappings.len(),
        success, errors, "stripe balance poll complete"
    );
}
