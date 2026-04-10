//! Minimal webhook receiver for local testing with `stripe listen`.
//!
//! Usage:
//!   STRIPE_SECRET_KEY=sk_test_... \
//!   STRIPE_WEBHOOK_SECRET=whsec_... \
//!   DATABASE_URL=postgres://... \
//!   cargo run -p billing --example test_webhook_server
//!
//! Then in another terminal:
//!   stripe listen --forward-to localhost:8080/api/v1/billing/webhooks/stripe
//!   stripe trigger checkout.session.completed

use billing::db::BillingDb;
use billing::http::{BillingHttpState, billing_router};
use billing::stripe::client::StripeClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info,billing=debug")
        .init();

    let stripe_key = std::env::var("STRIPE_SECRET_KEY").expect("set STRIPE_SECRET_KEY");
    let webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET")
        .expect("set STRIPE_WEBHOOK_SECRET (from `stripe listen` output)");
    let db_url =
        std::env::var("DATABASE_URL").expect("set DATABASE_URL to the vers-landing postgres URL");

    let stripe = StripeClient::new(&stripe_key)?;
    let db = BillingDb::connect(&db_url).await?;

    tracing::info!("connected to database");

    let state = BillingHttpState {
        stripe,
        db,
        webhook_secret,
    };

    let app = axum::Router::new().nest("/api/v1/billing", billing_router(state));

    let addr = "0.0.0.0:8080";
    tracing::info!("webhook test server listening on {addr}");
    tracing::info!("run: stripe listen --forward-to localhost:8080/api/v1/billing/webhooks/stripe");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
