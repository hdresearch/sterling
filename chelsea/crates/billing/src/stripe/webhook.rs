//! Stripe webhook event processing.
//!
//! Architecture: the webhook handler is split into two layers:
//!
//! 1. **`process_event`** — Pure decision logic. Takes parsed Stripe types,
//!    returns a [`WebhookOutcome`] describing what should happen. Only needs
//!    a `StripeClient` for org_id resolution (customer metadata lookup).
//!    No database dependency.
//!
//! 2. **`execute_outcome`** — Side-effect executor. Takes the outcome and
//!    applies it: DB upserts, credit grants, etc. This is where `BillingDb`
//!    is used.
//!
//! This separation means tests can verify all decision logic without any
//! database infrastructure.

use tracing::{error, info, warn};

use super::client::{
    StripeCheckoutSession, StripeClient, StripeEvent, StripeInvoice, StripeSubscription,
};
use crate::db::{BillingDb, UpsertSubscription};

// ─── Plan name → tier mapping ────────────────────────────────────────────────

pub fn plan_name_to_tier(name: &str) -> Option<&'static str> {
    match name {
        "Free Plan" => Some("free"),
        "HDR Base Plan" | "Starter" => Some("starter"),
        "Pro" => Some("pro"),
        "Team" => Some("team"),
        "Enterprise" => Some("enterprise"),
        _ => None,
    }
}

// ─── Outcome types ───────────────────────────────────────────────────────────

/// The result of processing a webhook event.
#[derive(Debug, Clone)]
pub struct WebhookResult {
    pub handled: bool,
    pub org_id: Option<String>,
    pub action: Option<String>,
}

/// Side effects that the webhook handler wants to apply.
#[derive(Debug, Clone, PartialEq)]
pub enum WebhookEffect {
    /// Upsert an org subscription in the database.
    UpsertSubscription {
        org_id: String,
        tier: String,
        status: String,
        billing_provider: String,
        customer_id: Option<String>,
        subscription_id: Option<String>,
        product_id: Option<String>,
        price_id: Option<String>,
        is_free_plan: bool,
    },
    /// Cancel an org's subscription.
    CancelSubscription { org_id: String },
    /// Clear pending tier adjustment.
    ClearPendingAdjustment { org_id: String },
    /// Grant Stripe credit to a customer.
    GrantCredits {
        customer_id: String,
        amount_cents: i64,
        org_id: String,
        reason: String,
    },
}

/// The full outcome: metadata + list of effects to apply.
#[derive(Debug, Clone)]
pub struct WebhookOutcome {
    pub handled: bool,
    pub org_id: Option<String>,
    pub action: Option<String>,
    pub effects: Vec<WebhookEffect>,
}

impl WebhookOutcome {
    fn handled(org_id: Option<String>, action: &str) -> Self {
        Self {
            handled: true,
            org_id,
            action: Some(action.to_string()),
            effects: Vec::new(),
        }
    }

    fn unhandled() -> Self {
        Self {
            handled: false,
            org_id: None,
            action: None,
            effects: Vec::new(),
        }
    }

    fn with_effects(mut self, effects: Vec<WebhookEffect>) -> Self {
        self.effects = effects;
        self
    }

    /// Convert to a WebhookResult (drops the effects list).
    pub fn into_result(self) -> WebhookResult {
        WebhookResult {
            handled: self.handled,
            org_id: self.org_id,
            action: self.action,
        }
    }
}

// ─── Main entry point ────────────────────────────────────────────────────────

/// Process a webhook event end-to-end: decide + execute.
pub async fn process_event(
    stripe: &StripeClient,
    db: &BillingDb,
    event: &StripeEvent,
) -> WebhookResult {
    let outcome = decide(stripe, event).await;
    execute_effects(stripe, db, &outcome.effects).await;
    outcome.into_result()
}

// ─── Decision layer (pure-ish, only needs Stripe for org resolution) ─────────

/// Determine what should happen for this webhook event.
/// Only calls Stripe for org_id/product resolution — never touches the DB.
pub async fn decide(stripe: &StripeClient, event: &StripeEvent) -> WebhookOutcome {
    match event.event_type.as_str() {
        "customer.subscription.created" => {
            match serde_json::from_value::<StripeSubscription>(event.data.object.clone()) {
                Ok(sub) => decide_subscription_created(stripe, &sub).await,
                Err(e) => {
                    error!(error = %e, "failed to parse subscription");
                    WebhookOutcome::handled(None, "parse_error")
                }
            }
        }
        "customer.subscription.updated" => {
            match serde_json::from_value::<StripeSubscription>(event.data.object.clone()) {
                Ok(sub) => decide_subscription_updated(stripe, &sub).await,
                Err(e) => {
                    error!(error = %e, "failed to parse subscription");
                    WebhookOutcome::handled(None, "parse_error")
                }
            }
        }
        "customer.subscription.deleted" => {
            match serde_json::from_value::<StripeSubscription>(event.data.object.clone()) {
                Ok(sub) => decide_subscription_deleted(stripe, &sub).await,
                Err(e) => {
                    error!(error = %e, "failed to parse subscription");
                    WebhookOutcome::handled(None, "parse_error")
                }
            }
        }
        "checkout.session.completed" => {
            match serde_json::from_value::<StripeCheckoutSession>(event.data.object.clone()) {
                Ok(session) => decide_checkout_completed(&session),
                Err(e) => {
                    error!(error = %e, "failed to parse checkout session");
                    WebhookOutcome::handled(None, "parse_error")
                }
            }
        }
        "invoice.paid" => {
            match serde_json::from_value::<StripeInvoice>(event.data.object.clone()) {
                Ok(invoice) => decide_invoice_paid(&invoice),
                Err(e) => {
                    error!(error = %e, "failed to parse invoice");
                    WebhookOutcome::handled(None, "parse_error")
                }
            }
        }
        "invoice.payment_failed" => {
            if let Ok(invoice) = serde_json::from_value::<StripeInvoice>(event.data.object.clone())
            {
                warn!(
                    invoice_id = %invoice.id,
                    customer = %invoice.customer,
                    "invoice payment failed"
                );
            }
            WebhookOutcome::handled(None, "invoice.payment_failed_logged")
        }
        other => {
            info!(event_type = other, "unhandled stripe webhook event");
            WebhookOutcome::unhandled()
        }
    }
}

// ─── Effect executor ─────────────────────────────────────────────────────────

/// Apply the side effects from a webhook outcome.
pub async fn execute_effects(stripe: &StripeClient, db: &BillingDb, effects: &[WebhookEffect]) {
    for effect in effects {
        match effect {
            WebhookEffect::UpsertSubscription {
                org_id,
                tier,
                status,
                billing_provider,
                customer_id,
                subscription_id,
                product_id,
                price_id,
                is_free_plan,
            } => {
                if let Err(e) = db
                    .upsert_org_subscription(UpsertSubscription {
                        org_id,
                        tier,
                        status,
                        billing_provider,
                        customer_id: customer_id.as_deref(),
                        subscription_id: subscription_id.as_deref(),
                        product_id: product_id.as_deref(),
                        price_id: price_id.as_deref(),
                        is_free_plan: *is_free_plan,
                    })
                    .await
                {
                    error!(org_id, error = %e, "failed to upsert subscription");
                }
            }
            WebhookEffect::CancelSubscription { org_id } => {
                if let Err(e) = db.cancel_org_subscription_by_org(org_id).await {
                    error!(org_id, error = %e, "failed to cancel subscription");
                }
            }
            WebhookEffect::ClearPendingAdjustment { org_id } => {
                if let Err(e) = db.clear_pending_adjustment(org_id).await {
                    error!(org_id, error = %e, "failed to clear pending adjustment");
                }
            }
            WebhookEffect::GrantCredits {
                customer_id,
                amount_cents,
                org_id,
                reason,
            } => {
                if let Err(e) = stripe
                    .create_credit_grant(
                        customer_id,
                        *amount_cents,
                        "paid",
                        &[("org_id", org_id.as_str()), ("reason", reason.as_str())],
                    )
                    .await
                {
                    error!(org_id, amount_cents, error = %e, "failed to grant credits");
                }
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn resolve_org_id(stripe: &StripeClient, sub: &StripeSubscription) -> Option<String> {
    if let Some(org_id) = sub.metadata.get("org_id") {
        if !org_id.is_empty() {
            return Some(org_id.clone());
        }
    }

    let customer_id = sub.customer_id()?;
    match stripe.get_customer(&customer_id).await {
        Ok(customer) => {
            if customer.deleted.unwrap_or(false) {
                return None;
            }
            customer.metadata.get("org_id").cloned()
        }
        Err(e) => {
            error!(customer_id, error = %e, "failed to resolve org_id from customer");
            None
        }
    }
}

async fn get_product_name(stripe: &StripeClient, sub: &StripeSubscription) -> Option<String> {
    let product_id = sub.product_id()?;
    match stripe.get_product(&product_id).await {
        Ok(product) => Some(product.name),
        Err(e) => {
            error!(product_id, error = %e, "failed to get product name");
            None
        }
    }
}

fn map_status(sub: &StripeSubscription) -> &'static str {
    if sub.cancel_at_period_end && sub.status == "active" {
        return "cancellation_scheduled";
    }
    match sub.status.as_str() {
        "active" => "active",
        "trialing" => "trialing",
        "past_due" | "unpaid" => "past_due",
        "canceled" => "canceled",
        _ => "canceled",
    }
}

// ─── Decision functions ──────────────────────────────────────────────────────

async fn decide_subscription_created(
    stripe: &StripeClient,
    sub: &StripeSubscription,
) -> WebhookOutcome {
    let org_id = resolve_org_id(stripe, sub).await;
    let product_name = get_product_name(stripe, sub).await;

    info!(
        subscription_id = %sub.id,
        org_id = ?org_id,
        status = %sub.status,
        "subscription created"
    );

    let mut effects = Vec::new();
    if let Some(ref oid) = org_id {
        let tier = product_name
            .as_deref()
            .and_then(plan_name_to_tier)
            .unwrap_or("free");

        effects.push(WebhookEffect::UpsertSubscription {
            org_id: oid.clone(),
            tier: tier.to_string(),
            status: "active".to_string(),
            billing_provider: "stripe".to_string(),
            customer_id: sub.customer_id(),
            subscription_id: Some(sub.id.clone()),
            product_id: sub.product_id(),
            price_id: sub.price_id(),
            is_free_plan: sub.is_free_plan(),
        });
    }

    WebhookOutcome::handled(org_id, "subscription_created").with_effects(effects)
}

async fn decide_subscription_updated(
    stripe: &StripeClient,
    sub: &StripeSubscription,
) -> WebhookOutcome {
    let org_id = resolve_org_id(stripe, sub).await;

    info!(
        subscription_id = %sub.id,
        org_id = ?org_id,
        status = %sub.status,
        cancel_at_period_end = sub.cancel_at_period_end,
        "subscription updated"
    );

    let mut effects = Vec::new();
    if let Some(ref oid) = org_id {
        let product_name = get_product_name(stripe, sub).await;
        let tier = product_name.as_deref().and_then(plan_name_to_tier);

        if let Some(tier) = tier {
            let status = map_status(sub);

            effects.push(WebhookEffect::UpsertSubscription {
                org_id: oid.clone(),
                tier: tier.to_string(),
                status: status.to_string(),
                billing_provider: "stripe".to_string(),
                customer_id: None,
                subscription_id: Some(sub.id.clone()),
                product_id: sub.product_id(),
                price_id: sub.price_id(),
                is_free_plan: sub.is_free_plan(),
            });
        }

        if !sub.cancel_at_period_end {
            effects.push(WebhookEffect::ClearPendingAdjustment {
                org_id: oid.clone(),
            });
        }
    }

    WebhookOutcome::handled(org_id, "subscription_updated").with_effects(effects)
}

async fn decide_subscription_deleted(
    stripe: &StripeClient,
    sub: &StripeSubscription,
) -> WebhookOutcome {
    let org_id = resolve_org_id(stripe, sub).await;

    info!(
        subscription_id = %sub.id,
        org_id = ?org_id,
        "subscription deleted"
    );

    let mut effects = Vec::new();
    if let Some(ref oid) = org_id {
        effects.push(WebhookEffect::CancelSubscription {
            org_id: oid.clone(),
        });
    }

    WebhookOutcome::handled(org_id, "subscription_deleted").with_effects(effects)
}

/// Checkout: no Stripe calls needed, pure decision from the session data.
pub fn decide_checkout_completed(session: &StripeCheckoutSession) -> WebhookOutcome {
    let org_id = session.metadata.get("org_id").cloned();

    info!(
        session_id = %session.id,
        org_id = ?org_id,
        mode = ?session.mode,
        payment_status = ?session.payment_status,
        "checkout session completed"
    );

    if session.mode.as_deref() != Some("payment")
        || session.payment_status.as_deref() != Some("paid")
    {
        return WebhookOutcome::handled(org_id, "checkout_session_ignored");
    }

    let Some(ref oid) = org_id else {
        warn!("checkout.session.completed without org_id in metadata");
        return WebhookOutcome::handled(None, "checkout_session_no_org");
    };

    let Some(customer_id) = session.customer_id() else {
        warn!("checkout.session.completed without customer");
        return WebhookOutcome::handled(org_id, "checkout_session_no_customer");
    };

    let amount = session.amount_total.unwrap_or(0);
    let mut effects = Vec::new();
    if amount > 0 {
        effects.push(WebhookEffect::GrantCredits {
            customer_id,
            amount_cents: amount,
            org_id: oid.clone(),
            reason: format!("Manual top-up via checkout (session {})", session.id),
        });
    }

    WebhookOutcome::handled(org_id, "checkout_credits_granted").with_effects(effects)
}

/// Invoice paid: no Stripe calls needed, pure decision from invoice data.
pub fn decide_invoice_paid(invoice: &StripeInvoice) -> WebhookOutcome {
    let org_id = invoice.metadata.get("org_id").cloned();
    let is_auto_topup = invoice
        .metadata
        .get("type")
        .map(|t| t == "auto_topup")
        .unwrap_or(false);

    info!(
        invoice_id = %invoice.id,
        org_id = ?org_id,
        is_auto_topup,
        amount_paid = ?invoice.amount_paid,
        "invoice paid"
    );

    if !is_auto_topup {
        return WebhookOutcome::handled(org_id, "invoice_paid_not_topup");
    }

    let Some(ref oid) = org_id else {
        warn!("invoice.paid auto_topup without org_id");
        return WebhookOutcome::handled(None, "invoice_paid_no_org");
    };

    let Some(customer_id) = invoice.customer_id() else {
        return WebhookOutcome::handled(org_id, "invoice_paid_no_customer");
    };

    let credit_cents = invoice
        .metadata
        .get("credit_cents")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| invoice.amount_paid.unwrap_or(0));

    let mut effects = Vec::new();
    if credit_cents > 0 {
        effects.push(WebhookEffect::GrantCredits {
            customer_id,
            amount_cents: credit_cents,
            org_id: oid.clone(),
            reason: format!("Auto top-up (invoice {})", invoice.id),
        });
    }

    WebhookOutcome::handled(org_id, "auto_topup_credits_granted").with_effects(effects)
}
