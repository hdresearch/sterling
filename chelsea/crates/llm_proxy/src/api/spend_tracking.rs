//! Post-request spend tracking: record logs, update spend counters, and send Stripe meter events.

use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppState;
use billing::stripe::meter::UsageEvent;

/// In-memory cache of team_id → stripe_customer_id mappings.
/// Populated lazily on first request per team, refreshed by the balance poller
/// (which queries all mappings anyway).
#[derive(Clone, Default)]
pub struct CustomerIdCache {
    inner: Arc<RwLock<HashMap<Uuid, Option<String>>>>,
}

impl CustomerIdCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get cached customer ID. Returns:
    /// - `Some(Some(id))` — cached, has Stripe billing
    /// - `Some(None)` — cached, no Stripe billing
    /// - `None` — not cached yet
    pub async fn get(&self, team_id: &Uuid) -> Option<Option<String>> {
        self.inner.read().await.get(team_id).cloned()
    }

    pub async fn set(&self, team_id: Uuid, customer_id: Option<String>) {
        self.inner.write().await.insert(team_id, customer_id);
    }
}

/// Record spend for a completed request.
///
/// This is called from a background task (spawned after the response is sent)
/// so it doesn't block the client. It:
/// 1. Updates the local spend counters (key + team)
/// 2. Sends a Stripe meter event if Stripe billing is configured
/// 3. Updates the local balance cache to keep gating accurate
pub async fn record_spend(
    state: &Arc<AppState>,
    key_id: Uuid,
    team_id: Option<Uuid>,
    cost: Decimal,
) {
    // 1. Local spend tracking (always — used for analytics even with Stripe)
    if let Err(e) = state.billing.increment_key_spend(key_id, cost).await {
        tracing::error!(key_id = %key_id, cost = %cost, "failed to record key spend: {e}");
    }
    if let Some(tid) = team_id {
        if let Err(e) = state.billing.increment_team_spend(tid, cost).await {
            tracing::error!(team_id = %tid, cost = %cost, "failed to record team spend: {e}");
        }
    }

    // 2. Stripe meter event (if configured)
    if let (Some(meter), Some(tid)) = (&state.meter, team_id) {
        let customer_id = resolve_stripe_customer(state, tid).await;

        if let Some(customer_id) = customer_id {
            meter.send(UsageEvent {
                stripe_customer_id: customer_id,
                cost,
            });

            // 3. Update local balance cache with pending spend
            if let Some(ref cache) = state.balance_cache {
                let millicents = cost_to_millicents(cost);
                cache.record_spend(&tid, millicents).await;
            }
        }
    }
}

/// Resolve team_id → Stripe customer ID with caching.
async fn resolve_stripe_customer(state: &Arc<AppState>, team_id: Uuid) -> Option<String> {
    // Check cache first
    if let Some(cached) = state.customer_id_cache.get(&team_id).await {
        return cached;
    }

    // Cache miss — query DB
    let result = match state.billing.get_stripe_customer_for_team(team_id).await {
        Ok(customer_id) => customer_id,
        Err(e) => {
            tracing::warn!(
                team_id = %team_id,
                error = %e,
                "failed to resolve Stripe customer for team"
            );
            return None;
        }
    };

    // Cache the result (including None, to avoid repeated DB misses)
    state.customer_id_cache.set(team_id, result.clone()).await;

    result
}

/// Convert cost in dollars to millicents (1 millicent = $0.001).
fn cost_to_millicents(cost: Decimal) -> i64 {
    use rust_decimal::prelude::ToPrimitive;
    let millicents = cost * Decimal::from(1000);
    millicents.ceil().to_i64().unwrap_or(0)
}
