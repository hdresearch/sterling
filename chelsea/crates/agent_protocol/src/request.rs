//! Request types sent from the host to the in-VM agent.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::base64_bytes;
use crate::types::ExecLogStream;

/// Request types sent from the host to the in-VM agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum AgentRequest {
    /// Health check ping.
    Ping,

    /// Check if the agent is ready and get VM info.
    Ready,

    /// Execute a command and collect output.
    Exec(ExecRequest),

    /// Execute a command and stream stdout/stderr incrementally.
    ExecStream(ExecRequest),

    /// Attach to an existing exec stream session.
    ExecStreamAttach(ExecStreamAttachRequest),

    /// Write a file in the VM.
    WriteFile(WriteFileRequest),

    /// Read a file from the VM.
    ReadFile(ReadFileRequest),

    /// Install an SSH public key for user access.
    InstallSshKey(InstallSshKeyRequest),

    /// Configure guest network settings.
    ConfigureNetwork(ConfigureNetworkRequest),

    /// Request graceful shutdown.
    Shutdown,

    /// Tail exec log entries.
    TailExecLog(TailExecLogRequest),

    /// Update the agent binary and restart.
    UpdateAgent(UpdateAgentRequest),

    /// Catch-all for forward compatibility. An older agent/host that receives
    /// a request type it doesn't recognise will deserialize it here rather
    /// than failing.
    #[serde(other)]
    Unknown,
}

/// Update the agent binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAgentRequest {
    /// URL to download the new agent binary from.
    pub url: String,

    /// Expected SHA-256 hex digest of the downloaded binary.
    /// The agent will refuse the update if the checksum doesn't match.
    pub sha256: String,

    /// Whether to restart the agent after a successful update.
    /// Restarting will drop the current vsock connection; the host
    /// should reconnect and wait for a new Ready event.
    #[serde(default = "default_restart")]
    pub restart: bool,
}

fn default_restart() -> bool {
    true
}

/// Execute a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    /// Command and arguments.
    pub command: Vec<String>,

    /// Optional exec identifier for tracking/correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec_id: Option<Uuid>,

    /// Environment variables to set.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Working directory.
    #[serde(default)]
    pub working_dir: Option<String>,

    /// Optional stdin data.
    #[serde(default)]
    pub stdin: Option<Vec<u8>>,

    /// Timeout in seconds (0 = no timeout).
    #[serde(default)]
    pub timeout_secs: u64,
}

/// Attach to an existing exec stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecStreamAttachRequest {
    /// Exec session to attach to.
    pub exec_id: Uuid,

    /// Resume after this cursor position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,

    /// If true, start from the latest retained event (ignores `cursor`).
    #[serde(default)]
    pub from_latest: bool,
}

/// Write a file in the VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileRequest {
    /// Destination path.
    pub path: String,

    /// File contents (base64 on the wire).
    #[serde(with = "base64_bytes")]
    pub content: Vec<u8>,

    /// File mode (e.g. 0o644).
    #[serde(default = "default_file_mode")]
    pub mode: u32,

    /// Create parent directories if missing.
    #[serde(default)]
    pub create_dirs: bool,
}

/// Read a file from the VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileRequest {
    /// Path to read.
    pub path: String,
}

/// Install an SSH public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallSshKeyRequest {
    /// SSH public key content.
    pub public_key: String,

    /// Username to install the key for.
    #[serde(default = "default_user")]
    pub user: String,
}

/// Configure guest network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigureNetworkRequest {
    /// IP address with CIDR notation (e.g. "192.168.0.2/24").
    pub ip_address: String,

    /// Gateway address.
    pub gateway: String,

    /// DNS servers.
    #[serde(default)]
    pub dns_servers: Vec<String>,
}

/// Tail exec log entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailExecLogRequest {
    /// Byte offset to start reading from.
    #[serde(default)]
    pub offset: u64,

    /// Maximum entries to return.
    #[serde(default = "default_max_log_entries")]
    pub max_entries: usize,

    /// Filter to a specific stream.
    #[serde(default)]
    pub stream: Option<ExecLogStream>,
}

fn default_file_mode() -> u32 {
    0o644
}

fn default_user() -> String {
    "root".to_string()
}

const fn default_max_log_entries() -> usize {
    100
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde::Serialize;
    use uuid::Uuid;

    use super::*;
    use crate::types::ExecLogStream;

    fn round_trip<T: Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
        let json = serde_json::to_string(value).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    // ── Unit-less variants ──────────────────────────────────────────

    #[test]
    fn ping() {
        let json = serde_json::to_string(&AgentRequest::Ping).unwrap();
        assert_eq!(json, r#"{"type":"Ping"}"#);
        assert!(matches!(
            serde_json::from_str::<AgentRequest>(&json).unwrap(),
            AgentRequest::Ping
        ));
    }

    #[test]
    fn ready() {
        let json = serde_json::to_string(&AgentRequest::Ready).unwrap();
        assert_eq!(json, r#"{"type":"Ready"}"#);
        assert!(matches!(
            serde_json::from_str::<AgentRequest>(&json).unwrap(),
            AgentRequest::Ready
        ));
    }

    #[test]
    fn shutdown() {
        let json = serde_json::to_string(&AgentRequest::Shutdown).unwrap();
        assert_eq!(json, r#"{"type":"Shutdown"}"#);
        assert!(matches!(
            serde_json::from_str::<AgentRequest>(&json).unwrap(),
            AgentRequest::Shutdown
        ));
    }

    // ── Exec ────────────────────────────────────────────────────────

    #[test]
    fn exec_round_trip() {
        let exec_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());

        let request = AgentRequest::Exec(ExecRequest {
            command: vec!["ls".to_string(), "-la".to_string()],
            exec_id: Some(exec_id),
            env,
            working_dir: Some("/tmp".to_string()),
            stdin: Some(vec![1, 2, 3]),
            timeout_secs: 30,
        });

        match round_trip(&request) {
            AgentRequest::Exec(req) => {
                assert_eq!(req.command, vec!["ls", "-la"]);
                assert_eq!(req.exec_id, Some(exec_id));
                assert_eq!(req.env.get("FOO").unwrap(), "bar");
                assert_eq!(req.working_dir.as_deref(), Some("/tmp"));
                assert_eq!(req.stdin, Some(vec![1, 2, 3]));
                assert_eq!(req.timeout_secs, 30);
            }
            _ => panic!("Expected Exec"),
        }
    }

    #[test]
    fn exec_defaults() {
        let json = r#"{"type":"Exec","payload":{"command":["echo","hi"]}}"#;
        match serde_json::from_str::<AgentRequest>(json).unwrap() {
            AgentRequest::Exec(req) => {
                assert_eq!(req.command, vec!["echo", "hi"]);
                assert_eq!(req.exec_id, None);
                assert!(req.env.is_empty());
                assert_eq!(req.working_dir, None);
                assert_eq!(req.stdin, None);
                assert_eq!(req.timeout_secs, 0);
            }
            _ => panic!("Expected Exec"),
        }
    }

    #[test]
    fn exec_id_omitted_when_none() {
        let request = AgentRequest::Exec(ExecRequest {
            command: vec!["ls".to_string()],
            exec_id: None,
            env: HashMap::new(),
            working_dir: None,
            stdin: None,
            timeout_secs: 0,
        });
        let json = serde_json::to_string(&request).unwrap();
        assert!(
            !json.contains("exec_id"),
            "exec_id should be skipped when None"
        );
    }

    #[test]
    fn exec_binary_stdin_round_trip() {
        let request = AgentRequest::Exec(ExecRequest {
            command: vec!["cat".to_string()],
            exec_id: None,
            env: HashMap::new(),
            working_dir: None,
            stdin: Some(vec![0x00, 0xFF, 0x80, 0x7F]),
            timeout_secs: 0,
        });
        match round_trip(&request) {
            AgentRequest::Exec(req) => {
                assert_eq!(req.stdin, Some(vec![0x00, 0xFF, 0x80, 0x7F]));
            }
            _ => panic!("Expected Exec"),
        }
    }

    // ── ExecStream ──────────────────────────────────────────────────

    #[test]
    fn exec_stream_round_trip() {
        let request = AgentRequest::ExecStream(ExecRequest {
            command: vec![
                "tail".to_string(),
                "-f".to_string(),
                "/var/log/syslog".to_string(),
            ],
            exec_id: None,
            env: HashMap::new(),
            working_dir: None,
            stdin: None,
            timeout_secs: 0,
        });
        assert!(matches!(round_trip(&request), AgentRequest::ExecStream(_)));
    }

    // ── ExecStreamAttach ────────────────────────────────────────────

    #[test]
    fn exec_stream_attach_round_trip() {
        let exec_id = Uuid::new_v4();
        let request = AgentRequest::ExecStreamAttach(ExecStreamAttachRequest {
            exec_id,
            cursor: Some(42),
            from_latest: false,
        });
        match round_trip(&request) {
            AgentRequest::ExecStreamAttach(req) => {
                assert_eq!(req.exec_id, exec_id);
                assert_eq!(req.cursor, Some(42));
                assert!(!req.from_latest);
            }
            _ => panic!("Expected ExecStreamAttach"),
        }
    }

    #[test]
    fn exec_stream_attach_defaults() {
        let id = Uuid::new_v4();
        let json = format!(
            r#"{{"type":"ExecStreamAttach","payload":{{"exec_id":"{}"}}}}"#,
            id
        );
        match serde_json::from_str::<AgentRequest>(&json).unwrap() {
            AgentRequest::ExecStreamAttach(req) => {
                assert_eq!(req.cursor, None);
                assert!(!req.from_latest);
            }
            _ => panic!("Expected ExecStreamAttach"),
        }
    }

    #[test]
    fn exec_stream_attach_from_latest() {
        let request = AgentRequest::ExecStreamAttach(ExecStreamAttachRequest {
            exec_id: Uuid::new_v4(),
            cursor: None,
            from_latest: true,
        });
        match round_trip(&request) {
            AgentRequest::ExecStreamAttach(req) => assert!(req.from_latest),
            _ => panic!("Expected ExecStreamAttach"),
        }
    }

    // ── WriteFile ───────────────────────────────────────────────────

    #[test]
    fn write_file_round_trip() {
        let request = AgentRequest::WriteFile(WriteFileRequest {
            path: "/etc/config.toml".to_string(),
            content: b"hello world".to_vec(),
            mode: 0o755,
            create_dirs: true,
        });
        match round_trip(&request) {
            AgentRequest::WriteFile(req) => {
                assert_eq!(req.path, "/etc/config.toml");
                assert_eq!(req.content, b"hello world");
                assert_eq!(req.mode, 0o755);
                assert!(req.create_dirs);
            }
            _ => panic!("Expected WriteFile"),
        }
    }

    #[test]
    fn write_file_content_is_base64_on_wire() {
        let request = AgentRequest::WriteFile(WriteFileRequest {
            path: "/tmp/f".to_string(),
            content: b"hello".to_vec(),
            mode: 0o644,
            create_dirs: false,
        });
        let json = serde_json::to_string(&request).unwrap();
        assert!(
            json.contains("aGVsbG8="),
            "content should be base64: {json}"
        );
        assert!(
            !json.contains("[104,"),
            "content should NOT be a byte array"
        );
    }

    #[test]
    fn write_file_binary_content_round_trip() {
        let request = AgentRequest::WriteFile(WriteFileRequest {
            path: "/tmp/bin".to_string(),
            content: vec![0x00, 0xFF, 0x80, 0x7F],
            mode: 0o644,
            create_dirs: false,
        });
        match round_trip(&request) {
            AgentRequest::WriteFile(req) => {
                assert_eq!(req.content, vec![0x00, 0xFF, 0x80, 0x7F]);
            }
            _ => panic!("Expected WriteFile"),
        }
    }

    #[test]
    fn write_file_defaults() {
        // "YWJj" is base64 for "abc"
        let json = r#"{"type":"WriteFile","payload":{"path":"/tmp/f","content":"YWJj"}}"#;
        match serde_json::from_str::<AgentRequest>(json).unwrap() {
            AgentRequest::WriteFile(req) => {
                assert_eq!(req.mode, 0o644);
                assert!(!req.create_dirs);
            }
            _ => panic!("Expected WriteFile"),
        }
    }

    // ── ReadFile ────────────────────────────────────────────────────

    #[test]
    fn read_file_round_trip() {
        let request = AgentRequest::ReadFile(ReadFileRequest {
            path: "/etc/hostname".to_string(),
        });
        match round_trip(&request) {
            AgentRequest::ReadFile(req) => assert_eq!(req.path, "/etc/hostname"),
            _ => panic!("Expected ReadFile"),
        }
    }

    // ── InstallSshKey ───────────────────────────────────────────────

    #[test]
    fn install_ssh_key_round_trip() {
        let request = AgentRequest::InstallSshKey(InstallSshKeyRequest {
            public_key: "ssh-ed25519 AAAA... user@host".to_string(),
            user: "ubuntu".to_string(),
        });
        match round_trip(&request) {
            AgentRequest::InstallSshKey(req) => {
                assert_eq!(req.public_key, "ssh-ed25519 AAAA... user@host");
                assert_eq!(req.user, "ubuntu");
            }
            _ => panic!("Expected InstallSshKey"),
        }
    }

    #[test]
    fn install_ssh_key_user_defaults_to_root() {
        let json = r#"{"type":"InstallSshKey","payload":{"public_key":"ssh-ed25519 AAAA..."}}"#;
        match serde_json::from_str::<AgentRequest>(json).unwrap() {
            AgentRequest::InstallSshKey(req) => assert_eq!(req.user, "root"),
            _ => panic!("Expected InstallSshKey"),
        }
    }

    // ── ConfigureNetwork ────────────────────────────────────────────

    #[test]
    fn configure_network_round_trip() {
        let request = AgentRequest::ConfigureNetwork(ConfigureNetworkRequest {
            ip_address: "192.168.0.2/24".to_string(),
            gateway: "192.168.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
        });
        match round_trip(&request) {
            AgentRequest::ConfigureNetwork(req) => {
                assert_eq!(req.ip_address, "192.168.0.2/24");
                assert_eq!(req.gateway, "192.168.0.1");
                assert_eq!(req.dns_servers, vec!["8.8.8.8", "1.1.1.1"]);
            }
            _ => panic!("Expected ConfigureNetwork"),
        }
    }

    #[test]
    fn configure_network_dns_defaults_empty() {
        let json = r#"{"type":"ConfigureNetwork","payload":{"ip_address":"10.0.0.2/24","gateway":"10.0.0.1"}}"#;
        match serde_json::from_str::<AgentRequest>(json).unwrap() {
            AgentRequest::ConfigureNetwork(req) => assert!(req.dns_servers.is_empty()),
            _ => panic!("Expected ConfigureNetwork"),
        }
    }

    // ── TailExecLog ─────────────────────────────────────────────────

    #[test]
    fn tail_exec_log_round_trip() {
        let request = AgentRequest::TailExecLog(TailExecLogRequest {
            offset: 1024,
            max_entries: 50,
            stream: Some(ExecLogStream::Stderr),
        });
        match round_trip(&request) {
            AgentRequest::TailExecLog(req) => {
                assert_eq!(req.offset, 1024);
                assert_eq!(req.max_entries, 50);
                assert_eq!(req.stream, Some(ExecLogStream::Stderr));
            }
            _ => panic!("Expected TailExecLog"),
        }
    }

    #[test]
    fn tail_exec_log_defaults() {
        let json = r#"{"type":"TailExecLog","payload":{}}"#;
        match serde_json::from_str::<AgentRequest>(json).unwrap() {
            AgentRequest::TailExecLog(req) => {
                assert_eq!(req.offset, 0);
                assert_eq!(req.max_entries, 100);
                assert_eq!(req.stream, None);
            }
            _ => panic!("Expected TailExecLog"),
        }
    }

    // ── Forward compatibility ───────────────────────────────────────

    #[test]
    fn unknown_type_no_payload_deserializes_to_unknown() {
        let json = r#"{"type":"FutureFeature"}"#;
        assert!(matches!(
            serde_json::from_str::<AgentRequest>(json).unwrap(),
            AgentRequest::Unknown
        ));
    }

    // NOTE: serde's #[serde(other)] with adjacently-tagged enums only
    // handles unit variants (no payload). Unknown types WITH a payload
    // still produce a parse error. This is a known serde limitation.
    #[test]
    fn unknown_type_with_payload_is_parse_error() {
        let json = r#"{"type":"FutureFeature","payload":{"foo":"bar"}}"#;
        assert!(serde_json::from_str::<AgentRequest>(json).is_err());
    }

    #[test]
    fn missing_type_field_is_rejected() {
        assert!(serde_json::from_str::<AgentRequest>(r#"{"command":["ls"]}"#).is_err());
    }

    // ── UpdateAgent ───────────────────────────────────────────────────

    #[test]
    fn update_agent_round_trip() {
        let request = AgentRequest::UpdateAgent(UpdateAgentRequest {
            url: "https://releases.example.com/chelsea-agent-v0.2.0".to_string(),
            sha256: "abcdef1234567890".to_string(),
            restart: true,
        });
        match round_trip(&request) {
            AgentRequest::UpdateAgent(req) => {
                assert_eq!(req.url, "https://releases.example.com/chelsea-agent-v0.2.0");
                assert_eq!(req.sha256, "abcdef1234567890");
                assert!(req.restart);
            }
            _ => panic!("Expected UpdateAgent"),
        }
    }

    #[test]
    fn update_agent_restart_defaults_to_true() {
        let json = r#"{"type":"UpdateAgent","payload":{"url":"https://example.com/agent","sha256":"abc123"}}"#;
        match serde_json::from_str::<AgentRequest>(json).unwrap() {
            AgentRequest::UpdateAgent(req) => {
                assert!(req.restart, "restart should default to true");
            }
            _ => panic!("Expected UpdateAgent"),
        }
    }

    #[test]
    fn update_agent_no_restart() {
        let request = AgentRequest::UpdateAgent(UpdateAgentRequest {
            url: "https://example.com/agent".to_string(),
            sha256: "abc123".to_string(),
            restart: false,
        });
        match round_trip(&request) {
            AgentRequest::UpdateAgent(req) => assert!(!req.restart),
            _ => panic!("Expected UpdateAgent"),
        }
    }

    // ── Wire format ─────────────────────────────────────────────────

    #[test]
    fn newline_delimited_protocol() {
        let messages = format!(
            "{}\n{}\n",
            serde_json::to_string(&AgentRequest::Ping).unwrap(),
            serde_json::to_string(&AgentRequest::Shutdown).unwrap(),
        );
        let parsed: Vec<AgentRequest> = messages
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(parsed.len(), 2);
        assert!(matches!(parsed[0], AgentRequest::Ping));
        assert!(matches!(parsed[1], AgentRequest::Shutdown));
    }
}
