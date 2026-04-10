//! Anthropic-compatible passthrough: POST /v1/messages

use std::sync::Arc;

use axum::{
    Extension, Router,
    body::Body,
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
};
use futures::StreamExt;
use http::StatusCode;
use serde_json::Value;

use crate::AppState;
use crate::api::spend_tracking;
use crate::auth::{self, AuthenticatedKey};
use crate::error::ProxyError;
use crate::request_id::RequestId;
use crate::spend;

pub fn routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/messages", post(messages))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            auth::auth_middleware,
        ))
}

async fn messages(
    State(state): State<Arc<AppState>>,
    Extension(auth_key): Extension<AuthenticatedKey>,
    Extension(RequestId(request_id)): Extension<RequestId>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ProxyError> {
    let start = std::time::Instant::now();

    let model = body["model"]
        .as_str()
        .ok_or(ProxyError::MissingField { field: "model" })?
        .to_string();

    let is_stream = body["stream"].as_bool().unwrap_or(false);

    if !auth_key.models.is_empty() && !auth_key.models.contains(&model) {
        return Err(ProxyError::ModelNotAllowed { model });
    }

    let routes = state
        .router
        .resolve(&model)
        .ok_or_else(|| ProxyError::ModelNotFound {
            model: model.clone(),
        })?;

    let mut last_error = String::new();
    for route in &routes {
        if is_stream {
            match route
                .provider
                .send_stream(
                    &state.http_client,
                    &route.credential,
                    &route.model_name,
                    body.clone(),
                )
                .await
            {
                Ok(streaming_resp) => {
                    let (state, auth_key, provider_name, model_name, request_body) = (
                        state.clone(),
                        auth_key.clone(),
                        route.provider_name.clone(),
                        route.model_name.clone(),
                        body.clone(),
                    );
                    let usage_rx = streaming_resp.usage_rx;
                    tokio::spawn(async move {
                        let usage = match usage_rx.await {
                            Ok(u) => {
                                if u.total_tokens == 0 {
                                    tracing::warn!(key_id = %auth_key.id, model = %model_name, "streaming usage returned zero tokens — spend may be under-counted");
                                }
                                u
                            }
                            Err(_) => {
                                tracing::warn!(key_id = %auth_key.id, model = %model_name, "streaming usage receiver dropped — spend will be under-counted");
                                Default::default()
                            }
                        };
                        let cost = spend::calculate_cost(&model_name, &usage);
                        if let Err(e) = state
                            .logs
                            .record_request(
                                request_id,
                                auth_key.id,
                                auth_key.team_id,
                                &model_name,
                                &provider_name,
                                usage.prompt_tokens as i32,
                                usage.completion_tokens as i32,
                                cost,
                                start.elapsed().as_millis() as i32,
                                "success",
                                None,
                                None,
                                &request_body,
                                &Value::Null,
                            )
                            .await
                        {
                            tracing::error!(key_id = %auth_key.id, "failed to record request log: {e}");
                        }
                        spend_tracking::record_spend(&state, auth_key.id, auth_key.team_id, cost)
                            .await;
                    });

                    let body_stream = streaming_resp
                        .stream
                        .map(|chunk| chunk.map_err(|e| axum::Error::new(e)));
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/event-stream")
                        .header("cache-control", "no-cache")
                        .body(Body::from_stream(body_stream))
                        .map_err(|_| ProxyError::StreamError);
                }
                Err(e) => {
                    last_error = format!("{e:#}");
                    continue;
                }
            }
        } else {
            match route
                .provider
                .send(
                    &state.http_client,
                    &route.credential,
                    &route.model_name,
                    body.clone(),
                )
                .await
            {
                Ok(resp) => {
                    let cost = spend::calculate_cost(&route.model_name, &resp.usage);
                    let (
                        state,
                        auth_key,
                        provider_name,
                        model_name,
                        response_body,
                        stop_reason,
                        usage,
                    ) = (
                        state.clone(),
                        auth_key.clone(),
                        route.provider_name.clone(),
                        route.model_name.clone(),
                        resp.body.clone(),
                        resp.stop_reason.clone(),
                        resp.usage.clone(),
                    );
                    tokio::spawn(async move {
                        if let Err(e) = state
                            .logs
                            .record_request(
                                request_id,
                                auth_key.id,
                                auth_key.team_id,
                                &model_name,
                                &provider_name,
                                usage.prompt_tokens as i32,
                                usage.completion_tokens as i32,
                                cost,
                                start.elapsed().as_millis() as i32,
                                "success",
                                stop_reason.as_deref(),
                                None,
                                &body,
                                &response_body,
                            )
                            .await
                        {
                            tracing::error!(key_id = %auth_key.id, "failed to record request log: {e}");
                        }
                        spend_tracking::record_spend(&state, auth_key.id, auth_key.team_id, cost)
                            .await;
                    });
                    return Ok((StatusCode::OK, axum::Json(resp.body)).into_response());
                }
                Err(e) => {
                    last_error = format!("{e:#}");
                    continue;
                }
            }
        }
    }

    Err(ProxyError::ProvidersFailed {
        model,
        detail: last_error,
    })
}
