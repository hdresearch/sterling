use axum::{
    extract::{FromRequestParts, Request},
    http::{HeaderMap, request::Parts},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use tracing::Instrument;
use uuid::Uuid;

/// Extractor for operation ID that is automatically injected by middleware
#[derive(Clone, Debug)]
pub struct OperationId(Arc<str>);

impl OperationId {
    /// Create a new operation ID with a generated UUID
    pub fn new() -> Self {
        Self(Arc::from(Uuid::new_v4().to_string()))
    }

    /// Create an operation ID from an existing string
    pub fn from_string(s: String) -> Self {
        Self(Arc::from(s))
    }

    /// Get the operation ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> FromRequestParts<S> for OperationId
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Extract from request extensions (injected by middleware)
        Ok(parts
            .extensions
            .get::<OperationId>()
            .cloned()
            .unwrap_or_else(OperationId::new))
    }
}

/// Middleware that injects a unique operation ID into each request and creates a tracing span
///
/// Checks for existing operation ID in headers (in order of preference):
/// 1. X-Request-ID
/// 2. X-Operation-ID
///
/// If no header is found, generates a new UUID
///
/// All downstream request processing happens within a tracing span that includes the operation_id
pub async fn operation_id_middleware(mut request: Request, next: Next) -> Response {
    let headers = request.headers();

    let operation_id = extract_operation_id_from_headers(headers).unwrap_or_else(OperationId::new);

    // Create a span that will encompass the entire request
    let span = tracing::info_span!(
        "http_request",
        operation_id = operation_id.as_str(),
        method = %request.method(),
        uri = %request.uri(),
    );

    // Insert into request extensions so handlers can access it if needed
    request.extensions_mut().insert(operation_id);

    // Run the request through the rest of the middleware/handler stack within the span
    next.run(request).instrument(span).await
}

/// Extract operation ID from request headers
fn extract_operation_id_from_headers(headers: &HeaderMap) -> Option<OperationId> {
    // Try X-Request-ID first
    if let Some(value) = headers.get("x-request-id") {
        if let Ok(s) = value.to_str() {
            return Some(OperationId::from_string(s.to_string()));
        }
    }

    // Fall back to X-Operation-ID
    if let Some(value) = headers.get("x-operation-id") {
        if let Ok(s) = value.to_str() {
            return Some(OperationId::from_string(s.to_string()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use tower::util::ServiceExt;
    use uuid::Uuid;

    async fn echo_operation_id(operation_id: OperationId) -> String {
        operation_id.as_str().to_owned()
    }

    fn router() -> Router {
        Router::new()
            .route("/", get(echo_operation_id))
            .layer(middleware::from_fn(operation_id_middleware))
    }

    #[test]
    fn prefers_x_request_id_header() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");

        runtime.block_on(async {
            let request = Request::builder()
                .uri("/")
                .header("x-request-id", "req-123")
                .header("x-operation-id", "op-456")
                .body(Body::empty())
                .expect("failed to build request");

            let response = router().oneshot(request).await.expect("request failed");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("failed to read body");

            assert_eq!(body.as_ref(), b"req-123");
        });
    }

    #[test]
    fn uses_x_operation_id_when_request_id_missing() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");

        runtime.block_on(async {
            let request = Request::builder()
                .uri("/")
                .header("x-operation-id", "op-789")
                .body(Body::empty())
                .expect("failed to build request");

            let response = router().oneshot(request).await.expect("request failed");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("failed to read body");

            assert_eq!(body.as_ref(), b"op-789");
        });
    }

    #[test]
    fn generates_uuid_when_no_headers_present() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");

        runtime.block_on(async {
            let request = Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("failed to build request");

            let response = router().oneshot(request).await.expect("request failed");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("failed to read body");

            let generated = std::str::from_utf8(&body).expect("response body not utf-8");
            assert!(
                Uuid::parse_str(generated).is_ok(),
                "expected UUID, got {generated}"
            );
        });
    }
}
