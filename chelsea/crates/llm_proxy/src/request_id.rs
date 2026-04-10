//! Request ID middleware.
//!
//! Generates a UUID for each request (or reads from incoming `x-request-id` header).
//! Attaches it to request extensions and returns it in the response header.

use axum::{extract::Request, middleware::Next, response::Response};
use uuid::Uuid;

/// The request ID attached to every request via extensions.
#[derive(Debug, Clone, Copy)]
pub struct RequestId(pub Uuid);

pub async fn request_id_middleware(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::new_v4);

    request.extensions_mut().insert(RequestId(id));

    let mut response = next.run(request).await;

    if let Ok(val) = id.to_string().parse() {
        response.headers_mut().insert("x-request-id", val);
    }

    response
}
