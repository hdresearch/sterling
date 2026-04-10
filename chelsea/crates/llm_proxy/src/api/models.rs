//! GET /v1/models — list available models (OpenAI-compatible format).

use std::sync::Arc;

use axum::{Router, extract::State, routing::get};
use serde_json::json;

use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/v1/models", get(list_models))
}

async fn list_models(State(state): State<Arc<AppState>>) -> axum::Json<serde_json::Value> {
    let models: Vec<serde_json::Value> = state
        .router
        .available_models()
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "owned_by": "llm-proxy",
            })
        })
        .collect();

    axum::Json(json!({
        "object": "list",
        "data": models,
    }))
}
