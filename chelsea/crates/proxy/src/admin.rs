use axum::{
    Json, Router,
    body::Body as AxumBody,
    extract::State,
    http::{Request as AxumRequest, StatusCode as AxumStatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response as AxumResponse},
    routing::get,
};
use orch_wg::WG;
use subtle::ConstantTimeEq;
use tower_http::trace::TraceLayer;
use vers_config::VersConfig;

use crate::metrics;

#[derive(Clone)]
pub struct AdminState {
    pub wg: WG,
    pub metrics: metrics::Metrics,
}

pub fn build_router(admin_api_key: String, wg: WG, metrics: metrics::Metrics) -> Router {
    let protected_routes = Router::new()
        .route("/admin/wireguard/peers", get(admin_wireguard_peers))
        .route("/admin/metrics", get(admin_metrics))
        .route("/admin/config", get(get_config))
        .layer(middleware::from_fn_with_state(
            admin_api_key,
            admin_api_key_middleware,
        ))
        .with_state(AdminState { wg, metrics });

    Router::new()
        .route("/health", get(health_check))
        .merge(protected_routes)
        .layer(TraceLayer::new_for_http())
}

async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "proxy"
    }))
}

async fn admin_wireguard_peers(State(state): State<AdminState>) -> impl IntoResponse {
    match state.wg.list_peers() {
        Ok(peers) => Json(peers).into_response(),
        Err(err) => {
            tracing::error!(error = ?err, "Failed to list WireGuard peers");
            AxumStatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn admin_metrics(State(state): State<AdminState>) -> impl IntoResponse {
    let body = state.metrics.detailed();
    ([(header::CONTENT_TYPE, "application/json")], body)
}

async fn get_config() -> Json<&'static VersConfig> {
    let config = VersConfig::global();
    Json(config)
}

async fn admin_api_key_middleware(
    State(expected_key): State<String>,
    req: AxumRequest<AxumBody>,
    next: Next,
) -> std::result::Result<AxumResponse, AxumStatusCode> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim());
    let provided = auth_header.and_then(|value| {
        value
            .strip_prefix("Bearer ")
            .map(|v| v.trim())
            .or(Some(value))
    });

    let provided = provided.or_else(|| {
        req.headers()
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.trim())
    });

    let is_match = provided
        .map(|candidate| expected_key.as_bytes().ct_eq(candidate.as_bytes()).into())
        .unwrap_or(false);

    if !is_match {
        return Err(AxumStatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}
