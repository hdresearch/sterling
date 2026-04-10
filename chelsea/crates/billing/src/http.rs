//! HTTP handlers for billing endpoints.
//!
//! These are axum handlers and router builders that can be mounted
//! in any axum application. The billing crate owns its routes;
//! host services just call `billing_router()` and nest/merge it.

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use std::sync::Arc;
use tracing::{info, warn};

use crate::db::BillingDb;
use crate::stripe::client::StripeClient;
use crate::stripe::webhook;

/// Shared state for billing HTTP handlers.
#[derive(Clone)]
pub struct BillingHttpState {
    pub stripe: StripeClient,
    pub db: BillingDb,
    pub webhook_secret: String,
}

/// Create the billing router with all billing HTTP endpoints.
///
/// Mount this in your axum app:
/// ```ignore
/// app.nest("/billing", billing::http::billing_router(state))
/// ```
pub fn billing_router(state: BillingHttpState) -> Router {
    Router::new()
        .route("/webhooks/stripe", post(handle_stripe_webhook))
        .with_state(Arc::new(state))
}

/// POST /webhooks/stripe
///
/// Receives Stripe webhook events, verifies the signature, and processes them.
async fn handle_stripe_webhook(
    State(state): State<Arc<BillingHttpState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Extract signature header
    let sig_header = match headers.get("stripe-signature") {
        Some(v) => match v.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "invalid signature header encoding");
            }
        },
        None => {
            return (StatusCode::BAD_REQUEST, "missing stripe-signature header");
        }
    };

    // Verify signature and parse event
    let event = match StripeClient::verify_webhook(&body, &sig_header, &state.webhook_secret) {
        Ok(event) => event,
        Err(e) => {
            warn!(error = %e, "webhook signature verification failed");
            return (StatusCode::UNAUTHORIZED, "invalid signature");
        }
    };

    info!(
        event_id = %event.id,
        event_type = %event.event_type,
        "processing stripe webhook"
    );

    // Process the event
    let result = webhook::process_event(&state.stripe, &state.db, &event).await;

    if let Some(ref action) = result.action {
        info!(
            event_id = %event.id,
            action = action,
            org_id = ?result.org_id,
            "webhook processed"
        );
    }

    (StatusCode::OK, "ok")
}
