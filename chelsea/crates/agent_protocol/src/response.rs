//! Response types sent from the in-VM agent back to the host.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::base64_bytes;
use crate::types::{Capability, ExecLogEntry, ExecLogStream};

/// Response types sent from the in-VM agent back to the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum AgentResponse {
    /// Response to Ping.
    Pong,

    /// Success with no additional data.
    Ok,

    /// Agent ready notification (sent unsolicited on new connections,
    /// or in response to a Ready request).
    Ready(ReadyResponse),

    /// Collected exec result.
    ExecResult(ExecResult),

    /// Streaming stdout/stderr chunk.
    ExecStreamChunk(ExecStreamChunk),

    /// Streaming exec completion.
    ExecStreamExit(ExecStreamExit),

    /// File content.
    FileContent(FileContentResponse),

    /// Error.
    Error(ErrorResponse),

    /// Exec log entries.
    ExecLogChunk(ExecLogChunkResponse),

    /// Catch-all for forward compatibility.
    #[serde(other)]
    Unknown,
}

/// Agent ready notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyResponse {
    /// VM ID as reported by the guest.
    #[serde(default)]
    pub vm_id: Option<String>,

    /// Agent version string.
    pub version: String,

    /// Capabilities this agent supports. The host should check for required
    /// capabilities before issuing requests that depend on them.
    ///
    /// Defaults to empty for backward compatibility with older agents that
    /// don't send this field.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

/// Collected command execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    /// Process exit code.
    pub exit_code: i32,

    /// Standard output bytes.
    pub stdout: Vec<u8>,

    /// Standard error bytes.
    pub stderr: Vec<u8>,
}

/// Streaming stdout/stderr chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecStreamChunk {
    /// Exec session this chunk belongs to.
    pub exec_id: Option<Uuid>,

    /// Monotonic sequence number within the session.
    pub cursor: u64,

    /// Which stream (stdout or stderr).
    pub stream: ExecLogStream,

    /// Output bytes.
    pub data: Vec<u8>,
}

/// Streaming exec completion event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecStreamExit {
    /// Exec session that completed.
    pub exec_id: Option<Uuid>,

    /// Final cursor value.
    pub cursor: u64,

    /// Process exit code.
    pub exit_code: i32,
}

/// File content (base64 on the wire).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContentResponse {
    /// File bytes (base64 on the wire).
    #[serde(with = "base64_bytes")]
    pub content: Vec<u8>,
}

/// Error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Machine-readable error code.
    pub code: String,

    /// Human-readable error message.
    pub message: String,
}

/// Exec log entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecLogChunkResponse {
    /// Log entries.
    pub entries: Vec<ExecLogEntry>,

    /// Byte offset for the next read.
    pub next_offset: u64,

    /// True if the end of the log was reached.
    pub eof: bool,
}

// ── Convenience methods ─────────────────────────────────────────────

impl AgentResponse {
    /// Create an error response.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error(ErrorResponse {
            code: code.into(),
            message: message.into(),
        })
    }

    /// Returns `true` if this is an error response.
    pub fn is_error(&self) -> bool {
        matches!(self, AgentResponse::Error(_))
    }

    /// Extracts the error message if this is an error response.
    pub fn error_message(&self) -> Option<&str> {
        match self {
            AgentResponse::Error(e) => Some(&e.message),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use uuid::Uuid;

    use super::*;
    use crate::types::{ExecLogEntry, ExecLogStream};

    fn round_trip<T: Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
        let json = serde_json::to_string(value).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    // ── Unit-less variants ──────────────────────────────────────────

    #[test]
    fn pong() {
        let json = serde_json::to_string(&AgentResponse::Pong).unwrap();
        assert_eq!(json, r#"{"type":"Pong"}"#);
        assert!(matches!(
            round_trip(&AgentResponse::Pong),
            AgentResponse::Pong
        ));
    }

    #[test]
    fn ok() {
        let json = serde_json::to_string(&AgentResponse::Ok).unwrap();
        assert_eq!(json, r#"{"type":"Ok"}"#);
        assert!(matches!(round_trip(&AgentResponse::Ok), AgentResponse::Ok));
    }

    // ── Ready ───────────────────────────────────────────────────────

    #[test]
    fn ready_round_trip() {
        let response = AgentResponse::Ready(ReadyResponse {
            vm_id: Some("abc-123".to_string()),
            version: "0.1.0".to_string(),
            capabilities: vec![Capability::Exec, Capability::ExecStream],
        });
        match round_trip(&response) {
            AgentResponse::Ready(r) => {
                assert_eq!(r.vm_id.as_deref(), Some("abc-123"));
                assert_eq!(r.version, "0.1.0");
                assert_eq!(
                    r.capabilities,
                    vec![Capability::Exec, Capability::ExecStream]
                );
            }
            _ => panic!("Expected Ready"),
        }
    }

    #[test]
    fn ready_no_vm_id() {
        let response = AgentResponse::Ready(ReadyResponse {
            vm_id: None,
            version: "0.1.0".to_string(),
            capabilities: vec![],
        });
        match round_trip(&response) {
            AgentResponse::Ready(r) => assert_eq!(r.vm_id, None),
            _ => panic!("Expected Ready"),
        }
    }

    #[test]
    fn ready_capabilities_default_to_empty() {
        // An older agent that doesn't send capabilities at all
        let json = r#"{"type":"Ready","payload":{"version":"0.0.1"}}"#;
        match serde_json::from_str::<AgentResponse>(json).unwrap() {
            AgentResponse::Ready(r) => {
                assert_eq!(r.version, "0.0.1");
                assert!(r.capabilities.is_empty());
            }
            _ => panic!("Expected Ready"),
        }
    }

    #[test]
    fn ready_with_all_capabilities() {
        let response = AgentResponse::Ready(ReadyResponse {
            vm_id: None,
            version: "0.1.0".to_string(),
            capabilities: Capability::all_known(),
        });
        match round_trip(&response) {
            AgentResponse::Ready(r) => {
                assert_eq!(r.capabilities.len(), 8);
                assert!(r.capabilities.contains(&Capability::Exec));
                assert!(r.capabilities.contains(&Capability::ExecStream));
                assert!(r.capabilities.contains(&Capability::FileTransfer));
                assert!(r.capabilities.contains(&Capability::SshKeyInstall));
                assert!(r.capabilities.contains(&Capability::ConfigureNetwork));
                assert!(r.capabilities.contains(&Capability::Shutdown));
                assert!(r.capabilities.contains(&Capability::AgentUpdate));
                assert!(r.capabilities.contains(&Capability::TailExecLog));
            }
            _ => panic!("Expected Ready"),
        }
    }

    #[test]
    fn ready_with_unknown_capability_from_newer_agent() {
        let json = r#"{"type":"Ready","payload":{"version":"99.0.0","capabilities":["exec","quantum_teleport","shutdown"]}}"#;
        match serde_json::from_str::<AgentResponse>(json).unwrap() {
            AgentResponse::Ready(r) => {
                assert_eq!(r.capabilities.len(), 3);
                assert_eq!(r.capabilities[0], Capability::Exec);
                assert_eq!(r.capabilities[1], Capability::Other);
                assert_eq!(r.capabilities[2], Capability::Shutdown);
            }
            _ => panic!("Expected Ready"),
        }
    }

    // ── ExecResult ──────────────────────────────────────────────────

    #[test]
    fn exec_result_round_trip() {
        let response = AgentResponse::ExecResult(ExecResult {
            exit_code: 1,
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
        });
        match round_trip(&response) {
            AgentResponse::ExecResult(r) => {
                assert_eq!(r.exit_code, 1);
                assert_eq!(r.stdout, b"out");
                assert_eq!(r.stderr, b"err");
            }
            _ => panic!("Expected ExecResult"),
        }
    }

    #[test]
    fn exec_result_empty_output() {
        match round_trip(&AgentResponse::ExecResult(ExecResult {
            exit_code: 0,
            stdout: vec![],
            stderr: vec![],
        })) {
            AgentResponse::ExecResult(r) => {
                assert!(r.stdout.is_empty());
                assert!(r.stderr.is_empty());
            }
            _ => panic!("Expected ExecResult"),
        }
    }

    #[test]
    fn exec_result_negative_exit_code() {
        match round_trip(&AgentResponse::ExecResult(ExecResult {
            exit_code: -1,
            stdout: vec![],
            stderr: vec![],
        })) {
            AgentResponse::ExecResult(r) => assert_eq!(r.exit_code, -1),
            _ => panic!("Expected ExecResult"),
        }
    }

    #[test]
    fn exec_result_binary_output() {
        let response = AgentResponse::ExecResult(ExecResult {
            exit_code: 0,
            stdout: vec![0x00, 0xFF, 0xFE],
            stderr: vec![0x80],
        });
        match round_trip(&response) {
            AgentResponse::ExecResult(r) => {
                assert_eq!(r.stdout, vec![0x00, 0xFF, 0xFE]);
                assert_eq!(r.stderr, vec![0x80]);
            }
            _ => panic!("Expected ExecResult"),
        }
    }

    // ── ExecStreamChunk ─────────────────────────────────────────────

    #[test]
    fn exec_stream_chunk_round_trip() {
        let id = Uuid::new_v4();
        let response = AgentResponse::ExecStreamChunk(ExecStreamChunk {
            exec_id: Some(id),
            cursor: 7,
            stream: ExecLogStream::Stdout,
            data: b"hello\n".to_vec(),
        });
        match round_trip(&response) {
            AgentResponse::ExecStreamChunk(c) => {
                assert_eq!(c.exec_id, Some(id));
                assert_eq!(c.cursor, 7);
                assert_eq!(c.stream, ExecLogStream::Stdout);
                assert_eq!(c.data, b"hello\n");
            }
            _ => panic!("Expected ExecStreamChunk"),
        }
    }

    #[test]
    fn exec_stream_chunk_stderr() {
        let response = AgentResponse::ExecStreamChunk(ExecStreamChunk {
            exec_id: None,
            cursor: 0,
            stream: ExecLogStream::Stderr,
            data: b"error!".to_vec(),
        });
        match round_trip(&response) {
            AgentResponse::ExecStreamChunk(c) => {
                assert_eq!(c.stream, ExecLogStream::Stderr);
                assert_eq!(c.exec_id, None);
            }
            _ => panic!("Expected ExecStreamChunk"),
        }
    }

    // ── ExecStreamExit ──────────────────────────────────────────────

    #[test]
    fn exec_stream_exit_round_trip() {
        let id = Uuid::new_v4();
        let response = AgentResponse::ExecStreamExit(ExecStreamExit {
            exec_id: Some(id),
            cursor: 99,
            exit_code: 0,
        });
        match round_trip(&response) {
            AgentResponse::ExecStreamExit(e) => {
                assert_eq!(e.exec_id, Some(id));
                assert_eq!(e.cursor, 99);
                assert_eq!(e.exit_code, 0);
            }
            _ => panic!("Expected ExecStreamExit"),
        }
    }

    #[test]
    fn exec_stream_exit_nonzero() {
        match round_trip(&AgentResponse::ExecStreamExit(ExecStreamExit {
            exec_id: None,
            cursor: 3,
            exit_code: 127,
        })) {
            AgentResponse::ExecStreamExit(e) => assert_eq!(e.exit_code, 127),
            _ => panic!("Expected ExecStreamExit"),
        }
    }

    // ── FileContent ─────────────────────────────────────────────────

    #[test]
    fn file_content_round_trip() {
        let response = AgentResponse::FileContent(FileContentResponse {
            content: b"hello world".to_vec(),
        });
        match round_trip(&response) {
            AgentResponse::FileContent(f) => assert_eq!(f.content, b"hello world"),
            _ => panic!("Expected FileContent"),
        }
    }

    #[test]
    fn file_content_is_base64_on_wire() {
        let response = AgentResponse::FileContent(FileContentResponse {
            content: b"hello".to_vec(),
        });
        let json = serde_json::to_string(&response).unwrap();
        assert!(
            json.contains("aGVsbG8="),
            "content should be base64: {json}"
        );
    }

    #[test]
    fn file_content_binary_round_trip() {
        let response = AgentResponse::FileContent(FileContentResponse {
            content: vec![0x00, 0xFF, 0x80],
        });
        match round_trip(&response) {
            AgentResponse::FileContent(f) => assert_eq!(f.content, vec![0x00, 0xFF, 0x80]),
            _ => panic!("Expected FileContent"),
        }
    }

    // ── Error ───────────────────────────────────────────────────────

    #[test]
    fn error_round_trip() {
        let response = AgentResponse::error("ENOENT", "file not found");
        match round_trip(&response) {
            AgentResponse::Error(e) => {
                assert_eq!(e.code, "ENOENT");
                assert_eq!(e.message, "file not found");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn error_helper_and_predicates() {
        let err = AgentResponse::error("X", "y");
        assert!(err.is_error());
        assert_eq!(err.error_message(), Some("y"));
        assert!(!AgentResponse::Pong.is_error());
        assert_eq!(AgentResponse::Pong.error_message(), None);
    }

    // ── ExecLogChunk ────────────────────────────────────────────────

    #[test]
    fn exec_log_chunk_round_trip() {
        let id = Uuid::new_v4();
        let response = AgentResponse::ExecLogChunk(ExecLogChunkResponse {
            entries: vec![
                ExecLogEntry {
                    exec_id: Some(id),
                    timestamp: "2026-03-02T18:00:00Z".to_string(),
                    stream: ExecLogStream::Stdout,
                    data: b"line1\n".to_vec(),
                },
                ExecLogEntry {
                    exec_id: Some(id),
                    timestamp: "2026-03-02T18:00:01Z".to_string(),
                    stream: ExecLogStream::Stderr,
                    data: b"warn\n".to_vec(),
                },
            ],
            next_offset: 512,
            eof: false,
        });
        match round_trip(&response) {
            AgentResponse::ExecLogChunk(c) => {
                assert_eq!(c.entries.len(), 2);
                assert_eq!(c.entries[0].stream, ExecLogStream::Stdout);
                assert_eq!(c.entries[0].data, b"line1\n");
                assert_eq!(c.entries[1].stream, ExecLogStream::Stderr);
                assert_eq!(c.next_offset, 512);
                assert!(!c.eof);
            }
            _ => panic!("Expected ExecLogChunk"),
        }
    }

    #[test]
    fn exec_log_chunk_empty_eof() {
        match round_trip(&AgentResponse::ExecLogChunk(ExecLogChunkResponse {
            entries: vec![],
            next_offset: 0,
            eof: true,
        })) {
            AgentResponse::ExecLogChunk(c) => {
                assert!(c.entries.is_empty());
                assert!(c.eof);
            }
            _ => panic!("Expected ExecLogChunk"),
        }
    }

    // ── Forward compatibility ───────────────────────────────────────

    #[test]
    fn unknown_type_no_payload_deserializes_to_unknown() {
        let json = r#"{"type":"FutureEvent"}"#;
        assert!(matches!(
            serde_json::from_str::<AgentResponse>(json).unwrap(),
            AgentResponse::Unknown
        ));
    }

    #[test]
    fn missing_type_field_is_rejected() {
        assert!(serde_json::from_str::<AgentResponse>(r#"{"exit_code":0}"#).is_err());
    }
}
