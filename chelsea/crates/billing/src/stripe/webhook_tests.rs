//! Tests for webhook decision logic.
//!
//! These test `decide()` and the pure decision functions directly.
//! No database needed — we only verify the returned effects.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::stripe::client::{StripeCheckoutSession, StripeInvoice};
    use crate::stripe::test_helpers::*;
    use crate::stripe::webhook::{self, WebhookEffect, plan_name_to_tier};

    // ─── plan_name_to_tier ───────────────────────────────────────────

    #[test]
    fn tier_mapping() {
        assert_eq!(plan_name_to_tier("Free Plan"), Some("free"));
        assert_eq!(plan_name_to_tier("Starter"), Some("starter"));
        assert_eq!(plan_name_to_tier("HDR Base Plan"), Some("starter"));
        assert_eq!(plan_name_to_tier("Pro"), Some("pro"));
        assert_eq!(plan_name_to_tier("Team"), Some("team"));
        assert_eq!(plan_name_to_tier("Enterprise"), Some("enterprise"));
        assert_eq!(plan_name_to_tier("Unknown"), None);
        assert_eq!(plan_name_to_tier(""), None);
    }

    // ─── Checkout (pure — no Stripe calls) ───────────────────────────

    #[test]
    fn checkout_payment_grants_credits() {
        let session: StripeCheckoutSession =
            serde_json::from_value(checkout_session_json("cs_1", "cus_abc", "org-1", 5000))
                .unwrap();

        let outcome = webhook::decide_checkout_completed(&session);
        assert_eq!(outcome.action.as_deref(), Some("checkout_credits_granted"));
        assert_eq!(outcome.org_id.as_deref(), Some("org-1"));
        assert_eq!(outcome.effects.len(), 1);
        assert!(matches!(
            &outcome.effects[0],
            WebhookEffect::GrantCredits { customer_id, amount_cents: 5000, org_id, .. }
            if customer_id == "cus_abc" && org_id == "org-1"
        ));
    }

    #[test]
    fn checkout_subscription_mode_ignored() {
        let session: StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_sub",
            "mode": "subscription",
            "payment_status": "paid",
            "customer": "cus_abc",
            "amount_total": 2900,
            "metadata": {"org_id": "org-1"}
        }))
        .unwrap();

        let outcome = webhook::decide_checkout_completed(&session);
        assert_eq!(outcome.action.as_deref(), Some("checkout_session_ignored"));
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn checkout_unpaid_ignored() {
        let session: StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_unpaid",
            "mode": "payment",
            "payment_status": "unpaid",
            "customer": "cus_abc",
            "amount_total": 1000,
            "metadata": {"org_id": "org-1"}
        }))
        .unwrap();

        let outcome = webhook::decide_checkout_completed(&session);
        assert_eq!(outcome.action.as_deref(), Some("checkout_session_ignored"));
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn checkout_no_org_id() {
        let session: StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_no_org",
            "mode": "payment",
            "payment_status": "paid",
            "customer": "cus_abc",
            "amount_total": 1000,
            "metadata": {}
        }))
        .unwrap();

        let outcome = webhook::decide_checkout_completed(&session);
        assert_eq!(outcome.action.as_deref(), Some("checkout_session_no_org"));
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn checkout_no_customer() {
        let session: StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_no_cust",
            "mode": "payment",
            "payment_status": "paid",
            "amount_total": 1000,
            "metadata": {"org_id": "org-1"}
        }))
        .unwrap();

        let outcome = webhook::decide_checkout_completed(&session);
        assert_eq!(
            outcome.action.as_deref(),
            Some("checkout_session_no_customer")
        );
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn checkout_zero_amount_no_grant() {
        let session: StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_zero",
            "mode": "payment",
            "payment_status": "paid",
            "customer": "cus_abc",
            "amount_total": 0,
            "metadata": {"org_id": "org-1"}
        }))
        .unwrap();

        let outcome = webhook::decide_checkout_completed(&session);
        assert_eq!(outcome.action.as_deref(), Some("checkout_credits_granted"));
        assert!(outcome.effects.is_empty(), "no grant for zero amount");
    }

    // ─── Invoice paid (pure — no Stripe calls) ──────────────────────

    #[test]
    fn invoice_auto_topup_grants_credits() {
        let invoice: StripeInvoice =
            serde_json::from_value(invoice_json("inv_1", "cus_abc", "org-1", 10000, true)).unwrap();

        let outcome = webhook::decide_invoice_paid(&invoice);
        assert_eq!(
            outcome.action.as_deref(),
            Some("auto_topup_credits_granted")
        );
        assert_eq!(outcome.effects.len(), 1);
        assert!(matches!(
            &outcome.effects[0],
            WebhookEffect::GrantCredits {
                amount_cents: 10000,
                ..
            }
        ));
    }

    #[test]
    fn invoice_not_topup_ignored() {
        let invoice: StripeInvoice =
            serde_json::from_value(invoice_json("inv_reg", "cus_abc", "org-1", 2900, false))
                .unwrap();

        let outcome = webhook::decide_invoice_paid(&invoice);
        assert_eq!(outcome.action.as_deref(), Some("invoice_paid_not_topup"));
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn invoice_uses_credit_cents_metadata() {
        let invoice: StripeInvoice = serde_json::from_value(json!({
            "id": "inv_meta",
            "customer": "cus_abc",
            "amount_paid": 5000,
            "metadata": {
                "type": "auto_topup",
                "org_id": "org-1",
                "credit_cents": "7500"
            }
        }))
        .unwrap();

        let outcome = webhook::decide_invoice_paid(&invoice);
        assert_eq!(outcome.effects.len(), 1);
        assert!(matches!(
            &outcome.effects[0],
            WebhookEffect::GrantCredits {
                amount_cents: 7500,
                ..
            }
        ));
    }

    #[test]
    fn invoice_topup_no_org_id() {
        let invoice: StripeInvoice = serde_json::from_value(json!({
            "id": "inv_no_org",
            "customer": "cus_abc",
            "amount_paid": 5000,
            "metadata": {"type": "auto_topup"}
        }))
        .unwrap();

        let outcome = webhook::decide_invoice_paid(&invoice);
        assert_eq!(outcome.action.as_deref(), Some("invoice_paid_no_org"));
        assert!(outcome.effects.is_empty());
    }

    // ─── Subscription events (need mock Stripe for org resolution) ───

    #[tokio::test]
    async fn subscription_created_with_org_in_metadata() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/products/",
            json!({"id": "prod_starter", "name": "Starter", "metadata": {}}),
        );

        let event = make_event(
            "customer.subscription.created",
            subscription_json("sub_1", "cus_abc", "active", Some("org-from-meta")),
        );

        let outcome = webhook::decide(&mock.client(), &event).await;
        assert_eq!(outcome.action.as_deref(), Some("subscription_created"));
        assert_eq!(outcome.org_id.as_deref(), Some("org-from-meta"));
        assert_eq!(outcome.effects.len(), 1);
        assert!(matches!(
            &outcome.effects[0],
            WebhookEffect::UpsertSubscription { org_id, tier, status, .. }
            if org_id == "org-from-meta" && tier == "starter" && status == "active"
        ));
    }

    #[tokio::test]
    async fn subscription_created_resolves_org_from_customer() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/customers/",
            json!({
                "id": "cus_abc",
                "email": "test@example.com",
                "metadata": {"org_id": "org-from-customer"}
            }),
        );
        mock.on(
            "/v1/products/",
            json!({"id": "prod_starter", "name": "Pro", "metadata": {}}),
        );

        let event = make_event(
            "customer.subscription.created",
            subscription_json("sub_2", "cus_abc", "active", None),
        );

        let outcome = webhook::decide(&mock.client(), &event).await;
        assert_eq!(outcome.org_id.as_deref(), Some("org-from-customer"));
        assert!(matches!(
            &outcome.effects[0],
            WebhookEffect::UpsertSubscription { tier, .. } if tier == "pro"
        ));
    }

    #[tokio::test]
    async fn subscription_updated_cancellation_scheduled() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/products/",
            json!({"id": "prod_starter", "name": "Starter", "metadata": {}}),
        );

        let mut sub_json = subscription_json("sub_3", "cus_abc", "active", Some("org-1"));
        sub_json["cancel_at_period_end"] = json!(true);

        let event = make_event("customer.subscription.updated", sub_json);

        let outcome = webhook::decide(&mock.client(), &event).await;
        assert_eq!(outcome.action.as_deref(), Some("subscription_updated"));

        // Should upsert with cancellation_scheduled status, but NO clear adjustment
        let upsert = outcome
            .effects
            .iter()
            .find(|e| matches!(e, WebhookEffect::UpsertSubscription { .. }));
        assert!(upsert.is_some());
        assert!(matches!(
            upsert.unwrap(),
            WebhookEffect::UpsertSubscription { status, .. } if status == "cancellation_scheduled"
        ));

        // cancel_at_period_end = true → should NOT clear pending adjustment
        let clear = outcome
            .effects
            .iter()
            .any(|e| matches!(e, WebhookEffect::ClearPendingAdjustment { .. }));
        assert!(!clear, "should not clear adjustment when canceling");
    }

    #[tokio::test]
    async fn subscription_updated_clears_pending_adjustment() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/products/",
            json!({"id": "prod_starter", "name": "Starter", "metadata": {}}),
        );

        let event = make_event(
            "customer.subscription.updated",
            subscription_json("sub_4", "cus_abc", "active", Some("org-1")),
        );

        let outcome = webhook::decide(&mock.client(), &event).await;

        // cancel_at_period_end = false → SHOULD clear pending adjustment
        let clear = outcome
            .effects
            .iter()
            .any(|e| matches!(e, WebhookEffect::ClearPendingAdjustment { .. }));
        assert!(clear, "should clear adjustment when not canceling");
    }

    #[tokio::test]
    async fn subscription_deleted_cancels() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/customers/",
            json!({"id": "cus_abc", "metadata": {"org_id": "org-del"}}),
        );

        let event = make_event(
            "customer.subscription.deleted",
            subscription_json("sub_5", "cus_abc", "canceled", None),
        );

        let outcome = webhook::decide(&mock.client(), &event).await;
        assert_eq!(outcome.action.as_deref(), Some("subscription_deleted"));
        assert_eq!(outcome.effects.len(), 1);
        assert!(matches!(
            &outcome.effects[0],
            WebhookEffect::CancelSubscription { org_id } if org_id == "org-del"
        ));
    }

    // ─── Dispatcher ──────────────────────────────────────────────────

    #[tokio::test]
    async fn unhandled_event() {
        let mock = MockStripeServer::start().await;
        let event = make_event("customer.updated", json!({"id": "cus_123"}));

        let outcome = webhook::decide(&mock.client(), &event).await;
        assert!(!outcome.handled);
        assert!(outcome.effects.is_empty());
    }

    #[tokio::test]
    async fn malformed_subscription_returns_parse_error() {
        let mock = MockStripeServer::start().await;
        let event = make_event(
            "customer.subscription.created",
            json!({"not_a_subscription": true}),
        );

        let outcome = webhook::decide(&mock.client(), &event).await;
        assert!(outcome.handled);
        assert_eq!(outcome.action.as_deref(), Some("parse_error"));
        assert!(outcome.effects.is_empty());
    }

    #[tokio::test]
    async fn payment_failed_logged() {
        let mock = MockStripeServer::start().await;
        let event = make_event(
            "invoice.payment_failed",
            invoice_json("inv_fail", "cus_abc", "org-1", 2900, false),
        );

        let outcome = webhook::decide(&mock.client(), &event).await;
        assert!(outcome.handled);
        assert_eq!(
            outcome.action.as_deref(),
            Some("invoice.payment_failed_logged")
        );
        assert!(outcome.effects.is_empty());
    }
}
