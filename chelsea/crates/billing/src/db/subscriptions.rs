//! Operations on `vers_landing.org_subscriptions` — Stripe customer mapping and subscription state.

use uuid::Uuid;

use super::{BillingDb, OrgSubscription, TeamStripeMapping, UpsertSubscription};
use crate::error::DbError;

impl BillingDb {
    /// Get all team → Stripe customer ID mappings for Stripe-billed teams.
    pub async fn get_stripe_team_mappings(&self) -> Result<Vec<TeamStripeMapping>, DbError> {
        let conn = self.pool.get().await?;
        let rows = conn
            .query(
                "SELECT t.id AS team_id, t.org_id,
                        os.flowglad_customer_id AS stripe_customer_id,
                        os.auto_topup_enabled,
                        os.auto_topup_threshold_cents,
                        os.auto_topup_amount_cents
                 FROM llm_teams t
                 JOIN vers_landing.org_subscriptions os ON os.org_id = t.org_id
                 WHERE os.billing_provider = 'stripe'
                   AND os.flowglad_customer_id IS NOT NULL
                   AND os.status IN ('active', 'trialing')",
                &[],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| TeamStripeMapping {
                team_id: r.get("team_id"),
                org_id: r.get("org_id"),
                stripe_customer_id: r.get("stripe_customer_id"),
                auto_topup_enabled: r.get("auto_topup_enabled"),
                auto_topup_threshold_cents: r.get("auto_topup_threshold_cents"),
                auto_topup_amount_cents: r.get("auto_topup_amount_cents"),
            })
            .collect())
    }

    /// Get the Stripe customer ID for a specific team.
    pub async fn get_stripe_customer_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Option<String>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT os.flowglad_customer_id
                 FROM llm_teams t
                 JOIN vers_landing.org_subscriptions os ON os.org_id = t.org_id
                 WHERE t.id = $1
                   AND os.billing_provider = 'stripe'
                   AND os.flowglad_customer_id IS NOT NULL",
                &[&team_id],
            )
            .await?;

        Ok(row.map(|r| r.get("flowglad_customer_id")))
    }

    /// Get the Stripe customer ID for an org.
    pub async fn get_stripe_customer_for_org(
        &self,
        org_id: Uuid,
    ) -> Result<Option<String>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT flowglad_customer_id
                 FROM vers_landing.org_subscriptions
                 WHERE org_id = $1
                   AND billing_provider = 'stripe'
                   AND flowglad_customer_id IS NOT NULL
                   AND status IN ('active', 'trialing')",
                &[&org_id],
            )
            .await?;

        Ok(row.map(|r| r.get("flowglad_customer_id")))
    }

    /// Get subscription info for an org.
    pub async fn get_org_subscription(
        &self,
        org_id: Uuid,
    ) -> Result<Option<OrgSubscription>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT org_id, tier, status,
                        COALESCE(billing_provider, 'flowglad') AS billing_provider,
                        flowglad_customer_id, flowglad_subscription_id,
                        auto_topup_enabled, auto_topup_threshold_cents, auto_topup_amount_cents
                 FROM vers_landing.org_subscriptions
                 WHERE org_id = $1",
                &[&org_id],
            )
            .await?;

        Ok(row.map(|r| OrgSubscription {
            org_id: r.get("org_id"),
            tier: r.get("tier"),
            status: r.get("status"),
            billing_provider: r.get("billing_provider"),
            stripe_customer_id: r.get("flowglad_customer_id"),
            stripe_subscription_id: r.get("flowglad_subscription_id"),
            auto_topup_enabled: r.get("auto_topup_enabled"),
            auto_topup_threshold_cents: r.get("auto_topup_threshold_cents"),
            auto_topup_amount_cents: r.get("auto_topup_amount_cents"),
        }))
    }

    /// Get all orgs with auto-topup enabled (for batch processing).
    pub async fn get_orgs_with_auto_topup(&self) -> Result<Vec<OrgSubscription>, DbError> {
        let conn = self.pool.get().await?;
        let rows = conn
            .query(
                "SELECT org_id, tier, status,
                        COALESCE(billing_provider, 'flowglad') AS billing_provider,
                        flowglad_customer_id, flowglad_subscription_id,
                        auto_topup_enabled, auto_topup_threshold_cents, auto_topup_amount_cents
                 FROM vers_landing.org_subscriptions
                 WHERE auto_topup_enabled = TRUE
                   AND flowglad_customer_id IS NOT NULL
                   AND billing_provider = 'stripe'
                   AND status IN ('active', 'trialing')",
                &[],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| OrgSubscription {
                org_id: r.get("org_id"),
                tier: r.get("tier"),
                status: r.get("status"),
                billing_provider: r.get("billing_provider"),
                stripe_customer_id: r.get("flowglad_customer_id"),
                stripe_subscription_id: r.get("flowglad_subscription_id"),
                auto_topup_enabled: r.get("auto_topup_enabled"),
                auto_topup_threshold_cents: r.get("auto_topup_threshold_cents"),
                auto_topup_amount_cents: r.get("auto_topup_amount_cents"),
            })
            .collect())
    }

    // ─── Write operations ────────────────────────────────────────────

    /// Upsert a subscription from webhook data.
    ///
    /// Uses the same conflict resolution logic as the TypeScript version:
    /// - Never downgrade active/trialing to canceled via upsert
    /// - COALESCE preserves existing values when new values are NULL
    pub async fn upsert_org_subscription(&self, p: UpsertSubscription<'_>) -> Result<(), DbError> {
        let conn = self.pool.get().await?;

        // Parse org_id as UUID
        let org_id: Uuid = p
            .org_id
            .parse()
            .map_err(|e| DbError::Query(format!("invalid org_id UUID: {e}")))?;

        let is_free = p.is_free_plan || p.tier == "free";

        conn.execute(
            "INSERT INTO vers_landing.org_subscriptions (
                org_id, tier, status, billing_provider,
                flowglad_customer_id, flowglad_subscription_id,
                flowglad_product_id, flowglad_price_id,
                is_free_plan, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
            ON CONFLICT (org_id) DO UPDATE SET
                tier = EXCLUDED.tier,
                status = CASE
                    WHEN vers_landing.org_subscriptions.status IN ('active', 'trialing')
                        AND EXCLUDED.status = 'canceled'
                    THEN vers_landing.org_subscriptions.status
                    ELSE EXCLUDED.status
                END,
                billing_provider = COALESCE(EXCLUDED.billing_provider, vers_landing.org_subscriptions.billing_provider),
                flowglad_customer_id = COALESCE(EXCLUDED.flowglad_customer_id, vers_landing.org_subscriptions.flowglad_customer_id),
                flowglad_subscription_id = COALESCE(EXCLUDED.flowglad_subscription_id, vers_landing.org_subscriptions.flowglad_subscription_id),
                flowglad_product_id = COALESCE(EXCLUDED.flowglad_product_id, vers_landing.org_subscriptions.flowglad_product_id),
                flowglad_price_id = COALESCE(EXCLUDED.flowglad_price_id, vers_landing.org_subscriptions.flowglad_price_id),
                is_free_plan = CASE
                    WHEN EXCLUDED.is_free_plan THEN true
                    WHEN EXCLUDED.tier = 'free' THEN true
                    ELSE EXCLUDED.is_free_plan
                END,
                updated_at = NOW()",
            &[
                &org_id,                  // $1
                &p.tier,                  // $2
                &p.status,                // $3
                &p.billing_provider,      // $4
                &p.customer_id,           // $5
                &p.subscription_id,       // $6
                &p.product_id,            // $7
                &p.price_id,              // $8
                &is_free,                 // $9
            ],
        )
        .await?;

        Ok(())
    }

    /// Cancel a subscription by setting status to 'canceled'.
    pub async fn cancel_org_subscription_by_org(&self, org_id: &str) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        let org_id: Uuid = org_id
            .parse()
            .map_err(|e| DbError::Query(format!("invalid org_id UUID: {e}")))?;

        conn.execute(
            "UPDATE vers_landing.org_subscriptions
             SET status = 'canceled', updated_at = NOW()
             WHERE org_id = $1",
            &[&org_id],
        )
        .await?;

        Ok(())
    }

    /// Clear pending tier adjustment.
    pub async fn clear_pending_adjustment(&self, org_id: &str) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        let org_id: Uuid = org_id
            .parse()
            .map_err(|e| DbError::Query(format!("invalid org_id UUID: {e}")))?;

        conn.execute(
            "UPDATE vers_landing.org_subscriptions
             SET pending_tier = NULL, scheduled_adjustment_at = NULL, updated_at = NOW()
             WHERE org_id = $1",
            &[&org_id],
        )
        .await?;

        Ok(())
    }

    /// Update auto-topup configuration for an org.
    pub async fn update_auto_topup(
        &self,
        org_id: Uuid,
        enabled: bool,
        threshold_cents: i64,
        amount_cents: i64,
    ) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        conn.execute(
            "UPDATE vers_landing.org_subscriptions
             SET auto_topup_enabled = $2,
                 auto_topup_threshold_cents = $3,
                 auto_topup_amount_cents = $4,
                 updated_at = NOW()
             WHERE org_id = $1",
            &[&org_id, &enabled, &threshold_cents, &amount_cents],
        )
        .await?;
        Ok(())
    }
}
