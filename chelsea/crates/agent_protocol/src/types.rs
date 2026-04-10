//! Shared types used by both requests and responses.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies stdout or stderr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecLogStream {
    Stdout,
    Stderr,
}

/// A single exec log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecLogEntry {
    /// Exec session that produced this entry.
    pub exec_id: Option<Uuid>,

    /// ISO-8601 timestamp.
    pub timestamp: String,

    /// Which stream.
    pub stream: ExecLogStream,

    /// Output bytes.
    pub data: Vec<u8>,
}

/// Well-known agent capabilities advertised in the [`ReadyResponse`](crate::ReadyResponse).
///
/// The host should check for required capabilities before issuing requests
/// that depend on them. Unknown capabilities (from a newer agent) deserialize
/// to [`Capability::Other`] so the host can ignore them gracefully.
///
/// # Adding a new capability
///
/// 1. Add a variant here.
/// 2. Include it in [`Capability::all_known`] (and the test).
/// 3. Add it to the agent's `advertised_capabilities()` in `handlers.rs`.
/// 4. (Optional) Have the host check for it before using the feature.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Basic command execution with collected output.
    Exec,

    /// Streaming command execution with cursor-based reattach.
    ExecStream,

    /// File read/write operations.
    FileTransfer,

    /// SSH public key installation.
    SshKeyInstall,

    /// Guest network configuration.
    ConfigureNetwork,

    /// Graceful VM shutdown.
    Shutdown,

    /// Exec log tailing.
    TailExecLog,

    /// Self-update: download a new agent binary from a URL and restart.
    AgentUpdate,

    /// Catch-all for capabilities added by a newer agent that this version
    /// of the protocol crate doesn't know about yet.
    #[serde(other)]
    Other,
}

impl Capability {
    /// Returns all capabilities known to this version of the protocol.
    /// Useful for agents that support everything.
    pub fn all_known() -> Vec<Capability> {
        vec![
            Capability::Exec,
            Capability::ExecStream,
            Capability::FileTransfer,
            Capability::SshKeyInstall,
            Capability::ConfigureNetwork,
            Capability::AgentUpdate,
            Capability::Shutdown,
            Capability::TailExecLog,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_log_stream_serialization() {
        assert_eq!(
            serde_json::to_string(&ExecLogStream::Stdout).unwrap(),
            r#""stdout""#
        );
        assert_eq!(
            serde_json::to_string(&ExecLogStream::Stderr).unwrap(),
            r#""stderr""#
        );
    }

    #[test]
    fn exec_log_stream_deserialization() {
        assert_eq!(
            serde_json::from_str::<ExecLogStream>(r#""stdout""#).unwrap(),
            ExecLogStream::Stdout
        );
        assert_eq!(
            serde_json::from_str::<ExecLogStream>(r#""stderr""#).unwrap(),
            ExecLogStream::Stderr
        );
    }

    #[test]
    fn capability_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&Capability::Exec).unwrap(),
            r#""exec""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::ExecStream).unwrap(),
            r#""exec_stream""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::FileTransfer).unwrap(),
            r#""file_transfer""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::SshKeyInstall).unwrap(),
            r#""ssh_key_install""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::ConfigureNetwork).unwrap(),
            r#""configure_network""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::Shutdown).unwrap(),
            r#""shutdown""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::TailExecLog).unwrap(),
            r#""tail_exec_log""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::AgentUpdate).unwrap(),
            r#""agent_update""#
        );
    }

    #[test]
    fn capability_deserializes_from_snake_case() {
        assert_eq!(
            serde_json::from_str::<Capability>(r#""exec""#).unwrap(),
            Capability::Exec
        );
        assert_eq!(
            serde_json::from_str::<Capability>(r#""exec_stream""#).unwrap(),
            Capability::ExecStream
        );
    }

    #[test]
    fn unknown_capability_deserializes_to_other() {
        assert_eq!(
            serde_json::from_str::<Capability>(r#""quantum_teleport""#).unwrap(),
            Capability::Other
        );
    }

    #[test]
    fn all_known_contains_every_variant() {
        let all = Capability::all_known();
        assert!(all.contains(&Capability::Exec));
        assert!(all.contains(&Capability::ExecStream));
        assert!(all.contains(&Capability::FileTransfer));
        assert!(all.contains(&Capability::SshKeyInstall));
        assert!(all.contains(&Capability::ConfigureNetwork));
        assert!(all.contains(&Capability::Shutdown));
        assert!(all.contains(&Capability::TailExecLog));
        assert!(all.contains(&Capability::AgentUpdate));
        assert!(!all.contains(&Capability::Other));
        assert_eq!(all.len(), 8);
    }
}
