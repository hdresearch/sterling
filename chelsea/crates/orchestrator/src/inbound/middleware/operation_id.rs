use axum::{
    extract::{FromRequestParts, Request},
    http::{HeaderMap, HeaderValue, request::Parts},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use tracing::Instrument;
use uuid::Uuid;

/// Header name for request ID
const REQUEST_ID_HEADER: &str = "x-request-id";

/// Maximum allowed length for request IDs to prevent log flooding
const MAX_REQUEST_ID_LENGTH: usize = 256;

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
/// All downstream request processing happens within a tracing span that includes the request_id.
/// The operation ID is also returned in the response X-Request-ID header for client correlation.
pub async fn operation_id_middleware(mut request: Request, next: Next) -> Response {
    let headers = request.headers();

    let operation_id = extract_operation_id_from_headers(headers).unwrap_or_else(OperationId::new);

    // Clone the operation_id so we can add it to the response header later
    let response_request_id = operation_id.as_str().to_owned();

    // Create a span that will encompass the entire request
    let span = tracing::info_span!(
        "http_request",
        request_id = operation_id.as_str(),
        method = %request.method(),
        uri = %request.uri(),
    );

    // Insert into request extensions so handlers can access it if needed
    request.extensions_mut().insert(operation_id);

    // Run the request through the rest of the middleware/handler stack within the span
    let mut response = next.run(request).instrument(span).await;

    // Add the request ID to response headers for client correlation
    if let Ok(header_value) = HeaderValue::from_str(&response_request_id) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER, header_value);
    }

    response
}

/// Extract operation ID from request headers, with validation
fn extract_operation_id_from_headers(headers: &HeaderMap) -> Option<OperationId> {
    // Try X-Request-ID first
    if let Some(value) = headers.get("x-request-id") {
        if let Ok(s) = value.to_str() {
            if is_valid_request_id(s) {
                return Some(OperationId::from_string(s.to_string()));
            }
        }
    }

    // Fall back to X-Operation-ID
    if let Some(value) = headers.get("x-operation-id") {
        if let Ok(s) = value.to_str() {
            if is_valid_request_id(s) {
                return Some(OperationId::from_string(s.to_string()));
            }
        }
    }

    None
}

/// Validate request ID: must be non-empty, printable ASCII, and within length limit.
/// This prevents log injection attacks and log flooding.
fn is_valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_REQUEST_ID_LENGTH && id.bytes().all(|b| b >= 0x20 && b < 0x7F)
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

    #[test]
    fn returns_request_id_in_response_header() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");

        runtime.block_on(async {
            let request = Request::builder()
                .uri("/")
                .header("x-request-id", "test-correlation-id")
                .body(Body::empty())
                .expect("failed to build request");

            let response = router().oneshot(request).await.expect("request failed");

            // Verify the X-Request-ID header is returned in the response
            let response_header = response
                .headers()
                .get("x-request-id")
                .expect("x-request-id header missing from response");
            assert_eq!(response_header.to_str().unwrap(), "test-correlation-id");
        });
    }

    #[test]
    fn returns_generated_uuid_in_response_header() {
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

            // Verify the X-Request-ID header is returned in the response
            let response_header = response
                .headers()
                .get("x-request-id")
                .expect("x-request-id header missing from response");
            let header_value = response_header.to_str().expect("header not valid string");

            // Should be a valid UUID
            assert!(
                Uuid::parse_str(header_value).is_ok(),
                "expected UUID in response header, got {header_value}"
            );
        });
    }

    #[test]
    fn rejects_oversized_request_id() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");

        runtime.block_on(async {
            // Create a request ID that exceeds the max length
            let oversized_id = "x".repeat(300);
            let request = Request::builder()
                .uri("/")
                .header("x-request-id", &oversized_id)
                .body(Body::empty())
                .expect("failed to build request");

            let response = router().oneshot(request).await.expect("request failed");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("failed to read body");

            // Should have generated a new UUID instead of using the oversized one
            let generated = std::str::from_utf8(&body).expect("response body not utf-8");
            assert!(
                Uuid::parse_str(generated).is_ok(),
                "expected UUID when oversized request ID provided, got {generated}"
            );
        });
    }

    // Note: HTTP itself rejects control characters in headers, so we can't test
    // control character rejection at the middleware level. The is_valid_request_id
    // unit tests cover this case for defense-in-depth if headers are manipulated internally.

    #[test]
    fn validation_accepts_valid_request_ids() {
        // Standard UUID format
        assert!(is_valid_request_id("550e8400-e29b-41d4-a716-446655440000"));
        // Alphanumeric with hyphens
        assert!(is_valid_request_id("my-request-123"));
        // Just alphanumeric
        assert!(is_valid_request_id("abc123"));
        // With underscores and dots
        assert!(is_valid_request_id("req_id.v1"));
    }

    #[test]
    fn validation_rejects_invalid_request_ids() {
        // Empty
        assert!(!is_valid_request_id(""));
        // Too long
        assert!(!is_valid_request_id(&"x".repeat(257)));
        // Contains newline
        assert!(!is_valid_request_id("test\ninjection"));
        // Contains carriage return
        assert!(!is_valid_request_id("test\rinjection"));
        // Contains tab
        assert!(!is_valid_request_id("test\tinjection"));
        // Contains null byte
        assert!(!is_valid_request_id("test\0injection"));
        // Contains DEL character
        assert!(!is_valid_request_id("test\x7Finjection"));
    }
}
