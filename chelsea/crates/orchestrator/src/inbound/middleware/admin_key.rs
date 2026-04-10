use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use reqwest::StatusCode;
use vers_config::VersConfig;

/// Axum middleware for checking whether or not the bearer token matches VersConfig::orchestrator().admin_api_key
pub async fn check_admin_key(request: Request, next: Next) -> Response {
    let auth_header = match request.headers().get("Authorization") {
        Some(value) => value,
        None => return StatusCode::FORBIDDEN.into_response(),
    };

    let auth_str = match auth_header.to_str() {
        Ok(value) => value,
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let bearer_token = match auth_str.split_whitespace().nth(1) {
        Some(value) => value,
        None => return StatusCode::UNAUTHORIZED.into_response(),
    };

    match bearer_token == VersConfig::orchestrator().admin_api_key {
        true => next.run(request).await,
        false => StatusCode::UNAUTHORIZED.into_response(),
    }
}
