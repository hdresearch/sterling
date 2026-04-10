use std::io;
use thiserror::Error;

/// Errors that can occur when communicating with the in-VM agent over vsock.
#[derive(Debug, Error)]
pub enum VsockError {
    /// Failed to connect to the vsock Unix socket.
    #[error("Failed to connect to vsock socket at {path}: {source}")]
    ConnectionFailed { path: String, source: io::Error },

    /// Connection attempt timed out.
    #[error("Connection to vsock timed out after {timeout_ms}ms")]
    ConnectionTimeout { timeout_ms: u64 },

    /// Firecracker vsock handshake failed.
    #[error("Vsock handshake failed: {0}")]
    HandshakeFailed(String),

    /// Failed to send or receive data.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// JSON serialization/deserialization error.
    #[error("Protocol error: {0}")]
    Protocol(#[from] serde_json::Error),

    /// The agent is not ready to accept requests.
    #[error("Agent not ready: {0}")]
    AgentNotReady(String),

    /// Received an unexpected response from the agent.
    #[error("Unexpected response: expected {expected}, got {actual}")]
    UnexpectedResponse { expected: String, actual: String },

    /// Request timed out waiting for response.
    #[error("Request timed out after {timeout_ms}ms")]
    RequestTimeout { timeout_ms: u64 },

    /// Agent returned an error.
    #[error("Agent error: {0}")]
    AgentError(String),
}
