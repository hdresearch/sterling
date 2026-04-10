//! Error types for the chelsea-agent.

use thiserror::Error;

/// Errors that can occur in the chelsea-agent.
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum AgentError {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Protocol error (malformed request, etc.)
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// File operation error
    #[error("File error: {0}")]
    File(String),

    /// Exec error
    #[error("Exec error: {0}")]
    Exec(String),

    /// VM ID not found
    #[error("VM ID not found at /etc/vm_id")]
    VmIdNotFound,
}

#[allow(dead_code)]
impl AgentError {
    /// Create a protocol error with a message.
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }

    /// Create a file error with a message.
    pub fn file(msg: impl Into<String>) -> Self {
        Self::File(msg.into())
    }

    /// Create an exec error with a message.
    pub fn exec(msg: impl Into<String>) -> Self {
        Self::Exec(msg.into())
    }
}
