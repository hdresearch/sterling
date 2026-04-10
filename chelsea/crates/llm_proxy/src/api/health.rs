//! GET /health — health check endpoint (unauthenticated).
//! Pings both database pools with a short timeout so load balancers get a fast response.
//! Returns 200 as long as the log DB (critical path) is reachable.

use std::sync::Arc;
use std::time::Duration;

use axum::{Router, extract::State, routing::get};
use http::StatusCode;
use serde_json::json;

use crate::AppState;

/// Maximum time to wait for each DB ping before considering it failed.
const PING_TIMEOUT: Duration = Duration::from_secs(3);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health))
}

async fn health(State(state): State<Arc<AppState>>) -> (StatusCode, axum::Json<serde_json::Value>) {
    let billing_ok = tokio::time::timeout(PING_TIMEOUT, state.billing.ping())
        .await
        .unwrap_or(false);
    let logs_ok = tokio::time::timeout(PING_TIMEOUT, state.logs.ping())
        .await
        .unwrap_or(false);

    // Log DB is the critical path — billing failures are degraded but not down.
    let status = if logs_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let health_str = if billing_ok && logs_ok {
        "ok"
    } else if logs_ok {
        "degraded"
    } else {
        "unavailable"
    };

    (
        status,
        axum::Json(json!({
            "status": health_str,
            "service": "llm_proxy",
            "checks": {
                "billing_db": if billing_ok { "ok" } else { "error" },
                "log_db": if logs_ok { "ok" } else { "error" },
            }
        })),
    )
}
