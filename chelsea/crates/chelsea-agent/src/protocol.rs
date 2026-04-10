//! Protocol types for the chelsea-agent.
//!
//! Re-exports from [`agent_protocol`] — the shared crate that is the single
//! source of truth for the host ↔ agent wire format.

pub use agent_protocol::{
    AgentRequest, AgentResponse, ExecLogChunkResponse, ExecLogEntry, ExecLogStream, ExecRequest,
    ExecResult, ExecStreamAttachRequest, ExecStreamChunk, ExecStreamExit, FileContentResponse,
    InstallSshKeyRequest, ReadFileRequest, ReadyResponse, TailExecLogRequest, WriteFileRequest,
};
