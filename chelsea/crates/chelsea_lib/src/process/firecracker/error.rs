use thiserror::Error;

#[derive(Debug, Error)]
pub enum FirecrackerApiError {
    #[error("Error while making Firecracker API request: {0:?}")]
    Request(#[from] reqwest::Error),
    #[error("Received error response: {status_code} {error_body}")]
    ResponseNotOk {
        status_code: reqwest::StatusCode,
        error_body: String,
    },
    #[error("Failed to parse response from Firecracker API: {0}")]
    ParseError(#[from] serde_json::Error),
}
