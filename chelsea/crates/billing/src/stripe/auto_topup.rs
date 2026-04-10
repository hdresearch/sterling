//! Auto top-up service.
//!
//! Checks credit balances for orgs with auto-topup enabled and triggers
//! invoice creation when balance drops below the configured threshold.
//!
//! Flow:
//!   1. Get credit balance from Stripe
//!   2. If below threshold → create a one-off invoice
//!   3. Stripe auto-charges the default payment method
//!   4. On `invoice.paid` webhook → grant credits (handled in webhook.rs)

use tracing::{error, info, warn};

use super::client::StripeClient;
use crate::db::BillingDb;

/// Result of a single org's top-up check.
#[derive(Debug)]
pub struct TopUpResult {
    pub triggered: bool,
    pub error: Option<String>,
}

/// Check a single org's credit balance and create an invoice if below threshold.
pub async fn check_and_topup(
    stripe: &StripeClient,
    org_id: &str,
    customer_id: &str,
    threshold_cents: i64,
    amount_cents: i64,
) -> TopUpResult {
    // 1. Check current balance
    let available = match stripe.get_credit_balance(customer_id).await {
        Ok(balance) => balance,
        Err(e) => {
            return TopUpResult {
                triggered: false,
                error: Some(format!("balance check failed: {e}")),
            };
        }
    };

    if available >= threshold_cents {
        return TopUpResult {
            triggered: false,
            error: None,
        };
    }

    info!(
        org_id,
        customer_id,
        available,
        threshold_cents,
        amount_cents,
        "balance below threshold, triggering auto top-up"
    );

    // 2. Create invoice item
    let description = format!(
        "Auto top-up: ${:.2} prepaid credits",
        amount_cents as f64 / 100.0
    );
    let metadata = [("type", "auto_topup"), ("org_id", org_id)];
    // We need amount_cents as a string for the credit_cents metadata
    let amount_str = amount_cents.to_string();
    let full_metadata = [
        ("type", "auto_topup"),
        ("org_id", org_id),
        ("credit_cents", amount_str.as_str()),
    ];

    if let Err(e) = stripe
        .create_invoice_item(customer_id, amount_cents, &description, &metadata)
        .await
    {
        return TopUpResult {
            triggered: false,
            error: Some(format!("invoice item creation failed: {e}")),
        };
    }

    // 3. Create and finalize invoice
    match stripe
        .create_and_finalize_invoice(customer_id, &full_metadata)
        .await
    {
        Ok(invoice_id) => {
            info!(
                org_id,
                invoice_id, amount_cents, "auto top-up invoice created"
            );
            TopUpResult {
                triggered: true,
                error: None,
            }
        }
        Err(e) => TopUpResult {
            triggered: false,
            error: Some(format!("invoice creation failed: {e}")),
        },
    }
}

/// Process all orgs with auto-topup enabled.
///
/// Returns (checked, triggered, errors).
pub async fn process_all_topups(stripe: &StripeClient, db: &BillingDb) -> (usize, usize, usize) {
    let orgs = match db.get_orgs_with_auto_topup().await {
        Ok(orgs) => orgs,
        Err(e) => {
            error!(error = %e, "failed to get orgs with auto-topup");
            return (0, 0, 1);
        }
    };

    let mut triggered = 0;
    let mut errors = 0;

    for org in &orgs {
        let Some(ref customer_id) = org.stripe_customer_id else {
            continue;
        };
        if org.billing_provider != "stripe" {
            continue;
        }

        let result = check_and_topup(
            stripe,
            &org.org_id.to_string(),
            customer_id,
            org.auto_topup_threshold_cents.into(),
            org.auto_topup_amount_cents.into(),
        )
        .await;

        if result.triggered {
            triggered += 1;
        }
        if result.error.is_some() {
            errors += 1;
            warn!(
                org_id = %org.org_id,
                error = ?result.error,
                "auto top-up error"
            );
        }
    }

    info!(
        checked = orgs.len(),
        triggered, errors, "auto top-up batch complete"
    );
    (orgs.len(), triggered, errors)
}

/// Spawn a background task that runs auto-topup checks on an interval.
pub fn spawn_auto_topup_task(
    stripe: StripeClient,
    db: BillingDb,
    interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(?interval, "auto top-up background task started");
        loop {
            tokio::time::sleep(interval).await;
            let (checked, triggered, errors) = process_all_topups(&stripe, &db).await;
            if triggered > 0 || errors > 0 {
                info!(checked, triggered, errors, "auto top-up cycle complete");
            }
        }
    })
}
