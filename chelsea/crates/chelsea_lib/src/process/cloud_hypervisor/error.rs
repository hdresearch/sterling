use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudHypervisorApiError {
    #[error("Error while making Cloud Hypervisor API request: {0:?}")]
    Request(#[from] reqwest::Error),
    #[error("Received error response: {status_code} {error_body}")]
    ResponseNotOk {
        status_code: reqwest::StatusCode,
        error_body: String,
    },
    #[error("Failed to parse response from Cloud Hypervisor API: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("Operation not supported")]
    OperationNotSupported,
}
