use std::collections::HashMap;

pub use chelsea_lib::{
    process_manager::{VmFirecrackerProcessCommitMetadata, VmProcessCommitMetadata},
    vm_manager::{
        commit::{VmCommitMetadata, VmConfigCommit},
        types::{VmState, VmSummary},
    },
    volume_manager::VmVolumeCommitMetadata,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// The request query parameters for POST /api/vm/{vm_id}/commit
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmCommitQuery {
    /// If set to true, then after commit, the VM will remain paused. Overrides the default behavior, which is to automatically resume the VM.
    pub keep_paused: Option<bool>,
    /// If true, return an error immediately if the VM is still booting. Default: false.
    pub skip_wait_boot: Option<bool>,
}

/// The request body for POST /api/vm/{vm_id}/commit
#[derive(ToSchema, Serialize, Deserialize, Debug, Default)]
pub struct VmCommitRequest {
    /// If provided, chelsea will use the requested commit UUID. Otherwise, it will generate a UUID itself.
    #[serde(default)]
    pub commit_id: Option<Uuid>,
    /// Optional human-readable name for the commit. Defaults to auto-generated name if not provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional description for the commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The response body for POST /api/vm/{vm_id}/commit
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmCommitResponse {
    /// The UUID of the newly-created commit
    pub commit_id: Uuid,
}

/// Response body for GET /api/vm
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmListAllResponse {
    /// A list of nodes, each of which is a "root VM" with one or more children
    pub vms: Vec<VmSummary>,
}

/// Response body for GET /api/vm/{vm_id}
pub type VmStatusResponse = VmSummary;

/// WireGuard configuration for a VM
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmWireGuardConfig {
    pub private_key: String,
    pub public_key: String,
    pub ipv6_address: String,

    pub proxy_public_key: String,
    pub proxy_ipv6_address: String,
    pub proxy_public_ip: String,

    pub wg_port: u16,
}

/// Query params for POST /api/vm/new
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmCreateQuery {
    /// If true, wait for the newly-created VM to finish booting before returning. Default: false.
    pub wait_boot: Option<bool>,
}

/// Request body for POST /api/vm/new
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmCreateRequest {
    /// The UUID to be assigned to the newly-created VM; if none is supplied, chelsea will generate one.
    pub vm_id: Option<Uuid>,
    pub vm_config: VmCreateVmConfig,
    /// Optional WireGuard configuration. If None, VM will not have WireGuard setup.
    pub wireguard: VmWireGuardConfig,
    /// Optional user-defined environment variables to write to /etc/environment at boot.
    #[serde(default)]
    pub env_vars: Option<HashMap<String, String>>,
}

/// Response body for POST /api/vm/new
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmCreateResponse {
    /// The UUID of the newly-created VM.
    pub vm_id: Uuid,
}

/// Struct representing configuration options common to all VMs
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmCreateVmConfig {
    /// The kernel name. Currently, must be 'default.bin'
    pub kernel_name: Option<String>,
    /// The filesystem base image name. Currently, must be 'default'
    pub image_name: Option<String>,
    /// How many vCPUs to allocate to this VM (and its children)
    pub vcpu_count: Option<u32>,
    /// The RAM size, in MiB.
    pub mem_size_mib: Option<u32>,
    /// The disk size, in MiB.
    pub fs_size_mib: Option<u32>,
    // Labels to attach to this VM
    pub labels: Option<HashMap<String, String>>,
}

/// Query params for PATCH /api/vm/{vm_id}/state
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmUpdateStateQuery {
    /// If true, error immediately if the VM is not finished booting. Defaults to false.
    pub skip_wait_boot: Option<bool>,
}

/// Request body for PATCH /api/vm/{vm_id}/state
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmUpdateStateRequest {
    /// The requested state for the VM
    pub state: VmUpdateStateEnum,
}

/// Possible options for the state requested in PATCH /api/vm/{vm_id}/state
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub enum VmUpdateStateEnum {
    Paused,
    Running,
}

/// Request body for POST /api/vm/from_commit
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmFromCommitRequest {
    /// The UUID to be assigned to the newly-created VM; if none is supplied, chelsea will generate one.
    pub vm_id: Option<Uuid>,
    /// The commit UUID used to start the VM
    pub commit_id: Uuid,
    pub wireguard: VmWireGuardConfig,
    /// Optional user-defined environment variables to write to /etc/environment at boot.
    #[serde(default)]
    pub env_vars: Option<HashMap<String, String>>,
}

/// Response body for POST /api/vm/from_commit
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmFromCommitResponse {
    /// The UUID of the newly-created VM.
    pub vm_id: Uuid,
}

/// Response body for GET /api/vm/{vm_id}/ssh_key
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmSshKeyResponse {
    /// Private SSH key in stringified OpenSSH format
    pub ssh_private_key: String,
    /// The SSH port that will be DNAT'd to the VM's netns (and, in turn, to its TAP device)
    pub ssh_port: u16,
}

/// Request body for POST /api/vm/{vm_id}/notify
#[derive(ToSchema, Serialize, Deserialize, Debug)]
#[serde(tag = "tag_name", content = "tag_value", rename_all = "snake_case")]
pub enum VmNotifyRequest {
    /// Indicates that the VM is ready (e.g. tag_value = "true"); note that this is currently a string.
    Ready(String),
}

/// Request body for PATCH /api/vm/{vm_id}/disk
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmResizeDiskRequest {
    /// The new disk size in MiB. Must be strictly greater than the current size.
    pub fs_size_mib: u32,
}

/// Query params for PATCH /api/vm/{vm_id}/disk
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmResizeDiskQuery {
    /// If true, return an error immediately if the VM is still booting. Default: false.
    pub skip_wait_boot: Option<bool>,
}

/// Query params for DELETE /api/vm/{vm_id}
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmDeleteQuery {
    /// If true, return an error immediately if the VM is still booting. Default: false.
    pub skip_wait_boot: Option<bool>,
}

/// Query params for POST /api/vm/{vm_id}/sleep
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmSleepQuery {
    /// If true, error immediately rather than waiting for a booting VM. Default: false.
    pub skip_wait_boot: Option<bool>,
}

/// Request body for POST /api/vm/{vm_id}/wake
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmWakeRequest {
    pub wireguard: VmWireGuardConfig,
}

// ─── Exec API types ──────────────────────────────────────────────────────────

/// Query params for exec-related endpoints.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone, Default)]
pub struct VmExecQuery {
    /// If true, return an error immediately if the VM is still booting. Default: false.
    pub skip_wait_boot: Option<bool>,
}

/// Request body for POST /api/vm/{vm_id}/exec
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmExecRequest {
    /// Command and arguments to execute.
    pub command: Vec<String>,
    /// Optional exec identifier for tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec_id: Option<Uuid>,
    /// Optional environment variables to set for the process.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    /// Optional working directory for the command.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Optional stdin payload passed to the command.
    #[serde(default)]
    pub stdin: Option<String>,
    /// Timeout in seconds (0 = no timeout).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Response body for POST /api/vm/{vm_id}/exec
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmExecResponse {
    /// Exit code returned by the command.
    pub exit_code: i32,
    /// UTF-8 decoded stdout (lossy).
    pub stdout: String,
    /// UTF-8 decoded stderr (lossy).
    pub stderr: String,
    /// Exec identifier associated with this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec_id: Option<Uuid>,
}

/// Request body for POST /api/vm/{vm_id}/exec/stream/attach
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmExecStreamAttachRequest {
    /// Identifier of the exec stream session to reattach to.
    pub exec_id: Uuid,
    /// Optional cursor to resume from (exclusive). If omitted, the full retained backlog is replayed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// Start streaming after the latest retained chunk (ignores cursor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_latest: Option<bool>,
}

/// Query params for GET /api/vm/{vm_id}/exec/logs
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone, Default)]
pub struct VmExecLogQuery {
    /// Byte offset into the log file to start reading from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Maximum number of entries to return (server applies caps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u32>,
    /// Filter by stream (stdout/stderr). Default: all streams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<VmExecLogStream>,
    /// Skip waiting for boot state (mirrors exec).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_wait_boot: Option<bool>,
}

/// Response for exec log tail requests.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmExecLogResponse {
    /// Returned log entries.
    pub entries: Vec<VmExecLogEntry>,
    /// Next byte offset to continue from.
    pub next_offset: u64,
    /// True when the end of file was reached.
    pub eof: bool,
}

/// Individual log entry describing emitted stdout/stderr chunk.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmExecLogEntry {
    pub exec_id: Option<Uuid>,
    pub timestamp: String,
    pub stream: VmExecLogStream,
    /// Base64-encoded bytes from stdout/stderr chunk.
    pub data_b64: String,
}

/// Streams available for exec logging.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VmExecLogStream {
    Stdout,
    Stderr,
}

/// Streaming exec events emitted over the exec stream endpoint (NDJSON lines).
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VmExecStreamEvent {
    Chunk {
        exec_id: Option<Uuid>,
        cursor: u64,
        stream: VmExecLogStream,
        data_b64: String,
    },
    Exit {
        exec_id: Option<Uuid>,
        cursor: u64,
        exit_code: i32,
    },
}

// ── File Transfer ────────────────────────────────────────────────────

/// Request body for PUT /api/vm/{vm_id}/files
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmWriteFileRequest {
    /// Destination path on the VM.
    pub path: String,
    /// File contents, base64-encoded.
    pub content_b64: String,
    /// File mode (e.g. 0644). Defaults to 0644 if omitted.
    #[serde(default = "default_file_mode")]
    pub mode: u32,
    /// Create parent directories if they don't exist.
    #[serde(default)]
    pub create_dirs: bool,
}

fn default_file_mode() -> u32 {
    0o644
}

/// Response body for GET /api/vm/{vm_id}/files
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct VmReadFileResponse {
    /// File contents, base64-encoded.
    pub content_b64: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_exec_request_minimal() {
        let json = r#"{"command":["ls","-la"]}"#;
        let req: VmExecRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, vec!["ls", "-la"]);
        assert!(req.exec_id.is_none());
        assert!(req.env.is_none());
        assert!(req.working_dir.is_none());
        assert!(req.stdin.is_none());
        assert!(req.timeout_secs.is_none());
    }

    #[test]
    fn test_vm_exec_request_full() {
        let json = r#"{
            "command": ["echo", "hello"],
            "exec_id": "550e8400-e29b-41d4-a716-446655440000",
            "env": {"FOO": "bar"},
            "working_dir": "/tmp",
            "stdin": "input data",
            "timeout_secs": 30
        }"#;
        let req: VmExecRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, vec!["echo", "hello"]);
        assert!(req.exec_id.is_some());
        assert_eq!(req.env.as_ref().unwrap()["FOO"], "bar");
        assert_eq!(req.working_dir.as_deref(), Some("/tmp"));
        assert_eq!(req.timeout_secs, Some(30));
    }

    #[test]
    fn test_vm_exec_response_serialization() {
        let resp = VmExecResponse {
            exit_code: 0,
            stdout: "hello\n".to_string(),
            stderr: String::new(),
            exec_id: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""exit_code":0"#));
        assert!(json.contains(r#""stdout":"hello\n""#));
        // exec_id should be omitted when None
        assert!(!json.contains("exec_id"));
    }

    #[test]
    fn test_vm_exec_response_with_exec_id() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let resp = VmExecResponse {
            exit_code: 1,
            stdout: String::new(),
            stderr: "error\n".to_string(),
            exec_id: Some(id),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deser: VmExecResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.exit_code, 1);
        assert_eq!(deser.exec_id, Some(id));
    }

    #[test]
    fn test_vm_exec_stream_attach_request() {
        let json = r#"{"exec_id":"550e8400-e29b-41d4-a716-446655440000","cursor":42}"#;
        let req: VmExecStreamAttachRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.cursor, Some(42));
        assert!(req.from_latest.is_none());
    }

    #[test]
    fn test_vm_exec_stream_event_chunk() {
        let event = VmExecStreamEvent::Chunk {
            exec_id: None,
            cursor: 5,
            stream: VmExecLogStream::Stdout,
            data_b64: "aGVsbG8=".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"chunk""#));
        assert!(json.contains(r#""stream":"stdout""#));
    }

    #[test]
    fn test_vm_exec_stream_event_exit() {
        let event = VmExecStreamEvent::Exit {
            exec_id: None,
            cursor: 10,
            exit_code: 0,
        };
        let json = serde_json::to_string(&event).unwrap();
        let deser: VmExecStreamEvent = serde_json::from_str(&json).unwrap();
        match deser {
            VmExecStreamEvent::Exit {
                exit_code, cursor, ..
            } => {
                assert_eq!(exit_code, 0);
                assert_eq!(cursor, 10);
            }
            _ => panic!("expected Exit variant"),
        }
    }

    #[test]
    fn test_vm_exec_log_query_defaults() {
        let json = "{}";
        let query: VmExecLogQuery = serde_json::from_str(json).unwrap();
        assert!(query.offset.is_none());
        assert!(query.max_entries.is_none());
        assert!(query.stream.is_none());
    }

    #[test]
    fn test_vm_exec_log_response() {
        let resp = VmExecLogResponse {
            entries: vec![VmExecLogEntry {
                exec_id: None,
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                stream: VmExecLogStream::Stderr,
                data_b64: "ZXJyb3I=".to_string(),
            }],
            next_offset: 100,
            eof: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deser: VmExecLogResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.entries.len(), 1);
        assert_eq!(deser.entries[0].stream, VmExecLogStream::Stderr);
        assert!(!deser.eof);
    }

    #[test]
    fn test_vm_exec_log_stream_serde() {
        assert_eq!(
            serde_json::to_string(&VmExecLogStream::Stdout).unwrap(),
            r#""stdout""#
        );
        assert_eq!(
            serde_json::to_string(&VmExecLogStream::Stderr).unwrap(),
            r#""stderr""#
        );
        let deser: VmExecLogStream = serde_json::from_str(r#""stdout""#).unwrap();
        assert_eq!(deser, VmExecLogStream::Stdout);
    }
}
