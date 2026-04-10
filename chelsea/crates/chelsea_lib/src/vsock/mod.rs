//! Vsock client for communicating with the in-VM chelsea agent.
//!
//! This module provides a client for sending management commands to VMs
//! over vsock (virtual sockets), which provides faster and more secure
//! host-to-guest communication compared to SSH.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Chelsea Node ──── vsock (guest CID, port 10789) ──── chelsea-agent │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Firecracker exposes vsock via a Unix domain socket. The connection protocol:
//! 1. Connect to the Unix socket at the vsock path
//! 2. Send: "CONNECT {port}\n"
//! 3. Receive: "OK {local_port}\n"
//! 4. Then bidirectional JSON-lines communication
//!
//! Protocol types are defined in the [`agent_protocol`] crate — the single
//! source of truth shared between the host client and the in-VM agent.
//!
//! # Example
//!
//! ```ignore
//! use chelsea_lib::vsock::VsockClient;
//! use std::time::Duration;
//!
//! let client = VsockClient::new("/path/to/vsock.sock");
//!
//! // Wait for the agent to be ready
//! client.wait_ready(Duration::from_secs(30)).await?;
//!
//! // Install an SSH key for user access
//! client.install_ssh_key("ssh-ed25519 AAAA... user@host").await?;
//!
//! // Execute a command
//! let result = client.exec(&["ls", "-la", "/tmp"]).await?;
//! println!("Exit code: {}", result.exit_code);
//! ```

pub mod client;
pub mod error;

pub use client::{ExecStreamConnection, ExecStreamEvent, VsockClient};
pub use error::VsockError;

// Re-export protocol types from the shared crate for convenience.
pub use agent_protocol::{
    AGENT_PORT, AgentRequest, AgentResponse, ConfigureNetworkRequest, ExecLogChunkResponse,
    ExecRequest, ExecResult, ExecStreamAttachRequest, ExecStreamChunk, ExecStreamExit,
    FileContentResponse, InstallSshKeyRequest, ReadFileRequest, ReadyResponse, TailExecLogRequest,
    UpdateAgentRequest, WriteFileRequest,
};
