//! Billing routes — delegates to the `billing` crate's HTTP handlers.

use billing::http::BillingHttpState;
use utoipa_axum::router::OpenApiRouter;

use crate::inbound::InboundState;

/// Build billing routes. Returns an empty router if Stripe is not configured.
pub fn billing_routes(state: &InboundState) -> OpenApiRouter {
    match &state.stripe_billing {
        Some(stripe) => {
            let billing_state = BillingHttpState {
                stripe: stripe.client.clone(),
                db: stripe.db.clone(),
                webhook_secret: stripe.webhook_secret.clone(),
            };
            OpenApiRouter::from(billing::http::billing_router(billing_state))
        }
        None => OpenApiRouter::new(),
    }
}
