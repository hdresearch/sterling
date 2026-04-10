//! Shared protocol types for chelsea-agent ↔ host communication over vsock.
//!
//! This crate is the **single source of truth** for the JSON-over-newline wire
//! protocol used between the Chelsea host (via Firecracker's vsock Unix socket)
//! and the in-VM chelsea-agent.
//!
//! # Wire Format
//!
//! Each message is a single line of JSON terminated by `\n`. Messages are
//! tagged with a `"type"` field and an optional `"payload"` field:
//!
//! ```json
//! {"type":"Ping"}
//! {"type":"Exec","payload":{"command":["ls","-la"]}}
//! ```
//!
//! # Versioning
//!
//! Unknown `type` values deserialize to [`AgentRequest::Unknown`] or
//! [`AgentResponse::Unknown`] rather than causing parse errors. This allows
//! independent rollout of new message types — an older agent will simply
//! respond with an error for unknown requests rather than crashing.

mod base64_bytes;
pub mod request;
pub mod response;
pub mod types;

/// Default vsock port for the chelsea agent.
pub const AGENT_PORT: u32 = 10789;

// Flat re-exports so callers can `use agent_protocol::AgentRequest` etc.
pub use request::{
    AgentRequest, ConfigureNetworkRequest, ExecRequest, ExecStreamAttachRequest,
    InstallSshKeyRequest, ReadFileRequest, TailExecLogRequest, UpdateAgentRequest,
    WriteFileRequest,
};
pub use response::{
    AgentResponse, ErrorResponse, ExecLogChunkResponse, ExecResult, ExecStreamChunk,
    ExecStreamExit, FileContentResponse, ReadyResponse,
};
pub use types::{Capability, ExecLogEntry, ExecLogStream};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_port() {
        assert_eq!(AGENT_PORT, 10789);
    }
}
