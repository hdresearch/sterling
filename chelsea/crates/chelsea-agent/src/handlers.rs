//! Request handlers for the chelsea-agent.
//!
//! The [`AgentHandler`] struct is the central dispatch and capability registry.
//! Capabilities are declared when the handler is constructed, and dispatch
//! uses match guards that check the registered set. This makes it mechanically
//! impossible to advertise a capability without a working handler, or to have
//! a handler that isn't advertised.

use std::collections::{HashMap, HashSet};
use std::io::{self, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use agent_protocol::Capability;
use base64::{Engine as _, engine::general_purpose};
use chrono::Utc;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::exec_stream::{ExecStreamEvent, ExecStreamSession, SessionError};
use crate::protocol::{
    AgentRequest, AgentResponse, ExecLogChunkResponse, ExecLogEntry, ExecLogStream, ExecRequest,
    ExecResult, ExecStreamAttachRequest, ExecStreamChunk, ExecStreamExit, FileContentResponse,
    ReadyResponse, TailExecLogRequest,
};

/// Agent version string.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Path to the system-wide environment file, written by Chelsea at boot.
const ETC_ENVIRONMENT: &str = "/etc/environment";

/// Path to the VM ID file.
const VM_ID_PATH: &str = "/etc/vm_id";

/// Path to the SSH authorized_keys file.
const SSH_AUTHORIZED_KEYS_PATH: &str = "/root/.ssh/authorized_keys";

/// Directory + file for exec logs.
const EXEC_LOG_DIR: &str = "/var/log/chelsea-agent";
const EXEC_LOG_PATH: &str = "/var/log/chelsea-agent/exec.log";
const MAX_COLLECTED_OUTPUT_BYTES: usize = 10 * 1024 * 1024; // 10 MiB
const TRUNCATION_MESSAGE: &[u8] =
    b"\n[chelsea-agent] output truncated at 10MB; download logs for full output\n";
static LOG_WRITE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

async fn send_agent_response<W>(
    writer: &mut BufWriter<W>,
    response: AgentResponse,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let response_json =
        serde_json::to_string(&response).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    writer.write_all(response_json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

/// Result of dispatching a request through [`AgentHandler::dispatch`].
pub enum DispatchResult {
    /// A response to send back to the host.
    Response(AgentResponse),
    /// The handler took over the writer (streaming). The connection loop
    /// should continue reading the next request without sending a response.
    Streaming,
}

/// Central dispatch and capability registry for the agent.
///
/// Capabilities are declared in [`AgentHandler::new`] and the dispatch
/// match in [`AgentHandler::dispatch`] uses guards that check the
/// registered set. This guarantees:
///
/// - **Handler without capability registered** → guard fails → `UNSUPPORTED`
/// - **Capability registered without match arm** → falls through → `UNSUPPORTED`
/// - **Match arm references missing handler fn** → compile error
///
/// All failure modes are safe: the agent never claims to support something
/// it can't actually handle.
pub struct AgentHandler {
    capabilities: HashSet<Capability>,
}

impl AgentHandler {
    /// Create a new handler with the capabilities this agent implements.
    ///
    /// **This is the single place where capabilities are registered.** Only
    /// add a capability here when the corresponding handler is fully wired
    /// up in [`Self::dispatch`]. VM snapshots preserve the capability list
    /// indefinitely, so accuracy matters.
    pub fn new() -> Self {
        let mut capabilities = HashSet::new();
        capabilities.insert(Capability::Exec);
        capabilities.insert(Capability::ExecStream);
        capabilities.insert(Capability::FileTransfer);
        capabilities.insert(Capability::SshKeyInstall);
        // Capability::ConfigureNetwork — not yet implemented
        capabilities.insert(Capability::Shutdown);
        capabilities.insert(Capability::AgentUpdate);
        capabilities.insert(Capability::TailExecLog);
        Self { capabilities }
    }

    /// Returns true if this agent supports the given capability.
    pub fn supports(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    /// Returns the capabilities this agent advertises.
    pub fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.iter().cloned().collect()
    }

    /// Build the Ready response with this agent's capabilities.
    pub async fn ready_response(&self) -> AgentResponse {
        let vm_id = match tokio::fs::read_to_string(VM_ID_PATH).await {
            Ok(id) => Some(id.trim().to_string()),
            Err(e) => {
                warn!("Could not read VM ID from {}: {}", VM_ID_PATH, e);
                None
            }
        };

        AgentResponse::Ready(ReadyResponse {
            vm_id,
            version: VERSION.to_string(),
            capabilities: self.capabilities(),
        })
    }

    /// Dispatch a request. Capability-gated handlers only fire if the
    /// capability is registered; everything else falls through to UNSUPPORTED.
    pub async fn dispatch<W>(
        &self,
        request: AgentRequest,
        writer: &mut BufWriter<W>,
    ) -> Result<DispatchResult, io::Error>
    where
        W: AsyncWrite + Unpin,
    {
        match request {
            // ── Protocol-level (no capability needed) ───────────────
            AgentRequest::Ping => {
                debug!("Handling Ping request");
                Ok(DispatchResult::Response(AgentResponse::Pong))
            }
            AgentRequest::Ready => {
                debug!("Handling Ready request");
                Ok(DispatchResult::Response(self.ready_response().await))
            }

            // ── Streaming handlers (take over the writer) ──────────
            AgentRequest::ExecStream(req) if self.supports(&Capability::ExecStream) => {
                handle_exec_stream(req, writer).await?;
                Ok(DispatchResult::Streaming)
            }
            AgentRequest::ExecStreamAttach(req) if self.supports(&Capability::ExecStream) => {
                handle_exec_stream_attach(req, writer).await?;
                Ok(DispatchResult::Streaming)
            }

            // ── Request-response handlers ───────────────────────────
            AgentRequest::Exec(req) if self.supports(&Capability::Exec) => {
                Ok(DispatchResult::Response(handle_exec(req).await))
            }
            AgentRequest::WriteFile(req) if self.supports(&Capability::FileTransfer) => {
                Ok(DispatchResult::Response(handle_write_file(req).await))
            }
            AgentRequest::ReadFile(req) if self.supports(&Capability::FileTransfer) => {
                Ok(DispatchResult::Response(handle_read_file(req).await))
            }
            AgentRequest::InstallSshKey(req) if self.supports(&Capability::SshKeyInstall) => {
                Ok(DispatchResult::Response(handle_install_ssh_key(req).await))
            }
            AgentRequest::ConfigureNetwork(req) if self.supports(&Capability::ConfigureNetwork) => {
                Ok(DispatchResult::Response(
                    handle_configure_network(req).await,
                ))
            }
            AgentRequest::Shutdown if self.supports(&Capability::Shutdown) => {
                Ok(DispatchResult::Response(handle_shutdown().await))
            }
            AgentRequest::TailExecLog(req) if self.supports(&Capability::TailExecLog) => {
                Ok(DispatchResult::Response(handle_tail_exec_log(req).await))
            }
            AgentRequest::UpdateAgent(req) if self.supports(&Capability::AgentUpdate) => {
                Ok(DispatchResult::Response(handle_update_agent(req).await))
            }

            // ── Catch-all: unsupported or unknown ───────────────────
            _ => Ok(DispatchResult::Response(AgentResponse::error(
                "UNSUPPORTED",
                "This agent does not support this request type",
            ))),
        }
    }
}

/// Parse `/etc/environment`-format content into key-value pairs.
///
/// The format is one `KEY=VALUE` per line. Empty lines and lines starting
/// with `#` are skipped. The first `=` splits key from value; subsequent
/// `=` characters are part of the value.
fn parse_env_file(content: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                vars.insert(key.to_string(), value.to_string());
            }
        }
    }
    vars
}

/// Read `/etc/environment` and return its `KEY=VALUE` pairs.
fn read_etc_environment() -> HashMap<String, String> {
    match std::fs::read_to_string(ETC_ENVIRONMENT) {
        Ok(content) => parse_env_file(&content),
        Err(_) => HashMap::new(),
    }
}

/// Apply environment variables to a command: first from `/etc/environment`,
/// then from the request's `env` map (which takes precedence).
fn apply_env_vars(cmd: &mut Command, request_env: &HashMap<String, String>) {
    let env_file_vars = read_etc_environment();
    for (key, value) in &env_file_vars {
        cmd.env(key, value);
    }
    // Request-level vars override /etc/environment
    for (key, value) in request_env {
        cmd.env(key, value);
    }
}

/// Handle an Exec request.
async fn handle_exec(req: ExecRequest) -> AgentResponse {
    debug!("Handling Exec request: {:?}", req.command);

    if req.command.is_empty() {
        return AgentResponse::error("INVALID_COMMAND", "Command cannot be empty");
    }

    let exec_id = req.exec_id.unwrap_or_else(Uuid::new_v4);
    let logger = ExecLogger::new(exec_id);

    let program = &req.command[0];
    let args = &req.command[1..];

    let mut cmd = Command::new(program);
    cmd.args(args);

    apply_env_vars(&mut cmd, &req.env);

    if let Some(ref working_dir) = req.working_dir {
        cmd.current_dir(working_dir);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(if req.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to spawn process: {}", e);
            return AgentResponse::error("SPAWN_ERROR", format!("Failed to spawn process: {}", e));
        }
    };

    if let Some(ref stdin_data) = req.stdin {
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(stdin_data).await {
                warn!("Failed to write to stdin: {}", e);
            }
        }
    }

    let stdout_future =
        read_and_log_stream(child.stdout.take(), ExecLogStream::Stdout, logger.clone());
    let stderr_future =
        read_and_log_stream(child.stderr.take(), ExecLogStream::Stderr, logger.clone());

    let wait_future = async {
        let status = if req.timeout_secs > 0 {
            tokio::time::timeout(Duration::from_secs(req.timeout_secs), child.wait())
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "command timed out"))??
        } else {
            child.wait().await?
        };
        Ok::<_, io::Error>(status)
    };
    let result = tokio::try_join!(wait_future, stdout_future, stderr_future);

    match result {
        Ok((status, stdout, stderr)) => {
            let exit_code = status.code().unwrap_or(-1);
            debug!("Exec completed with exit code {}", exit_code);

            AgentResponse::ExecResult(ExecResult {
                exit_code,
                stdout,
                stderr,
            })
        }
        Err(e) => {
            if e.kind() == io::ErrorKind::TimedOut {
                AgentResponse::error(
                    "TIMEOUT",
                    format!("Command timed out after {} seconds", req.timeout_secs),
                )
            } else {
                error!("Failed to execute command: {}", e);
                AgentResponse::error("EXEC_ERROR", format!("Failed to execute command: {}", e))
            }
        }
    }
}

pub async fn handle_exec_stream<W>(
    mut req: ExecRequest,
    writer: &mut BufWriter<W>,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    if req.command.is_empty() {
        return send_agent_response(
            writer,
            AgentResponse::error("INVALID_COMMAND", "Command cannot be empty"),
        )
        .await;
    }

    let exec_id = req.exec_id.unwrap_or_else(Uuid::new_v4);
    req.exec_id = Some(exec_id);
    let logger = ExecLogger::new(exec_id);

    let program = &req.command[0];
    let args = &req.command[1..];

    let mut cmd = Command::new(program);
    cmd.args(args);

    apply_env_vars(&mut cmd, &req.env);

    if let Some(ref working_dir) = req.working_dir {
        cmd.current_dir(working_dir);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(if req.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            send_agent_response(
                writer,
                AgentResponse::error("SPAWN_ERROR", format!("Failed to spawn process: {}", e)),
            )
            .await?;
            return Ok(());
        }
    };

    if let Some(ref stdin_data) = req.stdin {
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(stdin_data).await {
                warn!("Failed to write to stdin: {}", e);
            }
        }
    }

    let (tx, rx) = mpsc::unbounded_channel::<ExecStreamEvent>();

    let mut pump_handles = Vec::new();
    if let Some(h) = spawn_stream_pump(
        child.stdout.take(),
        ExecLogStream::Stdout,
        exec_id,
        logger.clone(),
        tx.clone(),
    ) {
        pump_handles.push(h);
    }
    if let Some(h) = spawn_stream_pump(
        child.stderr.take(),
        ExecLogStream::Stderr,
        exec_id,
        logger.clone(),
        tx.clone(),
    ) {
        pump_handles.push(h);
    }
    spawn_wait_for_exit(child, req.timeout_secs, exec_id, tx.clone(), pump_handles);
    drop(tx);

    let session = match ExecStreamSession::register(exec_id, rx).await {
        Ok(session) => session,
        Err(SessionError::AlreadyExists(existing)) => {
            send_agent_response(
                writer,
                AgentResponse::error(
                    "EXEC_ID_IN_USE",
                    format!("Exec stream with id {existing} already exists"),
                ),
            )
            .await?;
            return Ok(());
        }
        Err(err) => {
            send_agent_response(
                writer,
                AgentResponse::error(
                    "STREAM_ERROR",
                    format!("Failed to register session: {err:?}"),
                ),
            )
            .await?;
            return Ok(());
        }
    };

    stream_session(session, None, writer).await?;

    Ok(())
}

/// Attach to an existing exec stream session and resume streaming from the specified cursor.
pub async fn handle_exec_stream_attach<W>(
    req: ExecStreamAttachRequest,
    writer: &mut BufWriter<W>,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let session = match ExecStreamSession::get(&req.exec_id).await {
        Ok(session) => session,
        Err(SessionError::NotFound(missing)) => {
            send_agent_response(
                writer,
                AgentResponse::error(
                    "EXEC_STREAM_NOT_FOUND",
                    format!("Exec stream {} is not active", missing),
                ),
            )
            .await?;
            return Ok(());
        }
        Err(err) => {
            send_agent_response(
                writer,
                AgentResponse::error("STREAM_ERROR", format!("Failed to attach: {err:?}")),
            )
            .await?;
            return Ok(());
        }
    };

    let cursor = if req.from_latest {
        session.latest_cursor().await
    } else {
        req.cursor
    };

    stream_session(session, cursor, writer).await
}

/// Handle a WriteFile request.
async fn handle_write_file(req: crate::protocol::WriteFileRequest) -> AgentResponse {
    debug!("Handling WriteFile request: {}", req.path);

    let path = Path::new(&req.path);

    // Create parent directories if requested
    if req.create_dirs {
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                error!("Failed to create parent directories: {}", e);
                return AgentResponse::error(
                    "DIR_ERROR",
                    format!("Failed to create parent directories: {}", e),
                );
            }
        }
    }

    // Write the file (content is already decoded from base64 by serde)
    if let Err(e) = tokio::fs::write(path, &req.content).await {
        error!("Failed to write file: {}", e);
        return AgentResponse::error("WRITE_ERROR", format!("Failed to write file: {}", e));
    }

    // Set file permissions
    if let Err(e) =
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(req.mode)).await
    {
        warn!("Failed to set file permissions: {}", e);
        // Don't fail the request for permission errors
    }

    info!("Successfully wrote file: {}", req.path);
    AgentResponse::Ok
}

/// Handle a ReadFile request.
async fn handle_read_file(req: crate::protocol::ReadFileRequest) -> AgentResponse {
    debug!("Handling ReadFile request: {}", req.path);

    match tokio::fs::read(&req.path).await {
        Ok(bytes) => {
            debug!(
                "Successfully read file: {} ({} bytes)",
                req.path,
                bytes.len()
            );
            // content is Vec<u8>; serde will base64-encode on the wire
            AgentResponse::FileContent(FileContentResponse { content: bytes })
        }
        Err(e) => {
            error!("Failed to read file {}: {}", req.path, e);
            AgentResponse::error("READ_ERROR", format!("Failed to read file: {}", e))
        }
    }
}

/// Handle an InstallSshKey request.
async fn handle_install_ssh_key(req: crate::protocol::InstallSshKeyRequest) -> AgentResponse {
    debug!("Handling InstallSshKey request");

    let ssh_dir = Path::new(SSH_AUTHORIZED_KEYS_PATH).parent().unwrap();

    // Create .ssh directory if it doesn't exist
    if let Err(e) = tokio::fs::create_dir_all(ssh_dir).await {
        error!("Failed to create .ssh directory: {}", e);
        return AgentResponse::error(
            "DIR_ERROR",
            format!("Failed to create .ssh directory: {}", e),
        );
    }

    // Set directory permissions to 700
    if let Err(e) =
        tokio::fs::set_permissions(ssh_dir, std::fs::Permissions::from_mode(0o700)).await
    {
        warn!("Failed to set .ssh directory permissions: {}", e);
    }

    // Write the authorized_keys file
    let key_content = format!("{}\n", req.public_key.trim());
    if let Err(e) = tokio::fs::write(SSH_AUTHORIZED_KEYS_PATH, &key_content).await {
        error!("Failed to write authorized_keys: {}", e);
        return AgentResponse::error(
            "WRITE_ERROR",
            format!("Failed to write authorized_keys: {}", e),
        );
    }

    // Set file permissions to 600
    if let Err(e) = tokio::fs::set_permissions(
        SSH_AUTHORIZED_KEYS_PATH,
        std::fs::Permissions::from_mode(0o600),
    )
    .await
    {
        warn!("Failed to set authorized_keys permissions: {}", e);
    }

    info!("Successfully installed SSH key");
    AgentResponse::Ok
}

/// Handle a Shutdown request.
async fn handle_shutdown() -> AgentResponse {
    info!("Handling Shutdown request - initiating system shutdown");

    // Spawn shutdown command
    let result = Command::new("reboot").spawn();

    match result {
        Ok(_) => AgentResponse::Ok,
        Err(e) => {
            error!("Failed to initiate shutdown: {}", e);
            AgentResponse::error(
                "SHUTDOWN_ERROR",
                format!("Failed to initiate shutdown: {}", e),
            )
        }
    }
}

/// Handle ConfigureNetwork request.
///
/// Not yet implemented. The match arm in `AgentHandler::dispatch` references
/// this function, but `Capability::ConfigureNetwork` is not registered so
/// the guard will never pass. When implementing, add the capability to
/// `AgentHandler::new`.
async fn handle_configure_network(_req: agent_protocol::ConfigureNetworkRequest) -> AgentResponse {
    AgentResponse::error("NOT_IMPLEMENTED", "ConfigureNetwork is not yet implemented")
}

/// Handle UpdateAgent request: resolve the binary path, then delegate to the
/// core update logic.
async fn handle_update_agent(req: agent_protocol::UpdateAgentRequest) -> AgentResponse {
    let current_exe = match std::fs::read_link("/proc/self/exe") {
        Ok(path) => path,
        Err(e) => {
            error!("Failed to resolve /proc/self/exe: {}", e);
            return AgentResponse::error(
                "UPDATE_ERROR",
                format!("Cannot determine agent binary path: {}", e),
            );
        }
    };

    let result = perform_agent_update(&req.url, &req.sha256, &current_exe).await;

    if matches!(result, AgentResponse::Ok) && req.restart {
        schedule_restart();
    }

    result
}

/// Core update logic: download, validate checksum, and atomically replace
/// the binary at `target_path`. Separated from `handle_update_agent` so
/// tests can provide a temp path instead of `/proc/self/exe`.
async fn perform_agent_update(
    url: &str,
    expected_sha256: &str,
    target_path: &Path,
) -> AgentResponse {
    use sha2::{Digest, Sha256};

    info!(%url, "Downloading agent update");

    // Download the new binary
    let bytes = match reqwest::get(url).await {
        Ok(resp) if resp.status().is_success() => match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to read response body: {}", e);
                return AgentResponse::error(
                    "DOWNLOAD_ERROR",
                    format!("Failed to read response body: {}", e),
                );
            }
        },
        Ok(resp) => {
            let status = resp.status();
            error!(%status, "Download returned non-success status");
            return AgentResponse::error(
                "DOWNLOAD_ERROR",
                format!("Download failed with HTTP {}", status),
            );
        }
        Err(e) => {
            error!("Failed to download agent binary: {}", e);
            return AgentResponse::error("DOWNLOAD_ERROR", format!("Failed to download: {}", e));
        }
    };

    // Validate checksum
    let digest = hex::encode(Sha256::digest(&bytes));
    if digest != expected_sha256 {
        error!(
            expected = %expected_sha256,
            actual = %digest,
            "Checksum mismatch"
        );
        return AgentResponse::error(
            "CHECKSUM_MISMATCH",
            format!(
                "SHA-256 mismatch: expected {}, got {}",
                expected_sha256, digest
            ),
        );
    }

    // Write to temp file next to the target binary
    let parent = target_path.parent().unwrap_or(Path::new("/tmp"));
    let tmp_path = parent.join(".chelsea-agent.update.tmp");

    if let Err(e) = tokio::fs::write(&tmp_path, &bytes).await {
        error!("Failed to write temp binary: {}", e);
        return AgentResponse::error("UPDATE_ERROR", format!("Failed to write temp file: {}", e));
    }

    // Make executable
    if let Err(e) =
        tokio::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755)).await
    {
        error!("Failed to set executable permission: {}", e);
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return AgentResponse::error("UPDATE_ERROR", format!("Failed to chmod: {}", e));
    }

    // Atomic rename
    if let Err(e) = tokio::fs::rename(&tmp_path, &target_path).await {
        error!("Failed to rename binary: {}", e);
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return AgentResponse::error("UPDATE_ERROR", format!("Failed to replace binary: {}", e));
    }

    info!(path = %target_path.display(), "Agent binary updated successfully");
    AgentResponse::Ok
}

/// Schedule a restart of the agent process. Spawns a task that waits
/// briefly (to let the Ok response flush), then exec's the new binary.
fn schedule_restart() {
    info!("Scheduling restart...");
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let exe = match std::fs::read_link("/proc/self/exe") {
            Ok(p) => p,
            Err(e) => {
                error!("Cannot resolve binary for restart: {}", e);
                return;
            }
        };
        let args: Vec<String> = std::env::args().collect();
        info!(exe = %exe.display(), "Restarting agent");
        match Command::new(&exe).args(&args[1..]).spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => error!("Failed to restart: {}", e),
        }
    });
}

/// Handle TailExecLog request.
async fn handle_tail_exec_log(req: TailExecLogRequest) -> AgentResponse {
    match read_exec_log(req).await {
        Ok(chunk) => AgentResponse::ExecLogChunk(chunk),
        Err(e) => {
            error!("Failed to read exec log: {}", e);
            AgentResponse::error("LOG_ERROR", format!("Failed to read exec log: {}", e))
        }
    }
}

#[derive(Clone)]
struct ExecLogger {
    exec_id: Uuid,
}

impl ExecLogger {
    fn new(exec_id: Uuid) -> Self {
        Self { exec_id }
    }

    async fn log_chunk(&self, stream: ExecLogStream, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        let record = PersistedExecLogRecord {
            exec_id: Some(self.exec_id),
            timestamp: Utc::now().to_rfc3339(),
            stream,
            data_b64: general_purpose::STANDARD.encode(data),
        };

        if let Err(e) = append_exec_log(record).await {
            warn!("Failed to append exec log: {}", e);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedExecLogRecord {
    exec_id: Option<Uuid>,
    timestamp: String,
    stream: ExecLogStream,
    data_b64: String,
}

async fn append_exec_log(record: PersistedExecLogRecord) -> io::Result<()> {
    let serialized =
        serde_json::to_vec(&record).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let _guard = LOG_WRITE_LOCK.lock().await;
    tokio::fs::create_dir_all(EXEC_LOG_DIR).await?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(EXEC_LOG_PATH)
        .await?;
    file.write_all(&serialized).await?;
    file.write_all(b"\n").await?;
    Ok(())
}

async fn read_and_log_stream<T>(
    reader: Option<T>,
    stream: ExecLogStream,
    logger: ExecLogger,
) -> io::Result<Vec<u8>>
where
    T: tokio::io::AsyncRead + Unpin,
{
    let mut collected = Vec::new();
    let mut truncated = false;

    if let Some(mut reader) = reader {
        let mut buf = [0u8; 4096];
        loop {
            let read = reader.read(&mut buf).await?;
            if read == 0 {
                break;
            }
            if collected.len() < MAX_COLLECTED_OUTPUT_BYTES {
                let remaining = MAX_COLLECTED_OUTPUT_BYTES - collected.len();
                let to_copy = remaining.min(read);
                collected.extend_from_slice(&buf[..to_copy]);
                if to_copy < read {
                    truncated = true;
                }
            } else {
                truncated = true;
            }
            logger.log_chunk(stream.clone(), &buf[..read]).await;
        }
    }

    if truncated {
        if collected.len() + TRUNCATION_MESSAGE.len() > MAX_COLLECTED_OUTPUT_BYTES {
            let keep = MAX_COLLECTED_OUTPUT_BYTES.saturating_sub(TRUNCATION_MESSAGE.len());
            collected.truncate(keep);
        }
        collected.extend_from_slice(TRUNCATION_MESSAGE);
    }

    Ok(collected)
}

async fn read_exec_log(req: TailExecLogRequest) -> io::Result<ExecLogChunkResponse> {
    let mut next_offset = req.offset;
    let mut entries = Vec::new();
    let mut reached_eof = true;

    let file = match tokio::fs::File::open(EXEC_LOG_PATH).await {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(ExecLogChunkResponse {
                entries,
                next_offset: 0,
                eof: true,
            });
        }
        Err(e) => return Err(e),
    };

    let mut file = file;
    file.seek(SeekFrom::Start(req.offset)).await?;
    let mut reader = BufReader::new(file);

    while entries.len() < req.max_entries {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            reached_eof = true;
            break;
        } else {
            reached_eof = false;
        }
        next_offset += bytes as u64;
        if line.trim().is_empty() {
            continue;
        }

        let record: PersistedExecLogRecord = match serde_json::from_str(line.trim_end_matches('\n'))
        {
            Ok(rec) => rec,
            Err(e) => {
                warn!("Failed to parse exec log line: {}", e);
                continue;
            }
        };

        if req.stream.as_ref().map_or(true, |s| *s == record.stream) {
            match general_purpose::STANDARD.decode(record.data_b64.as_bytes()) {
                Ok(data) => {
                    entries.push(ExecLogEntry {
                        exec_id: record.exec_id,
                        timestamp: record.timestamp,
                        stream: record.stream,
                        data,
                    });
                }
                Err(e) => warn!("Failed to decode exec log payload: {}", e),
            }
        }
    }

    Ok(ExecLogChunkResponse {
        entries,
        next_offset,
        eof: reached_eof,
    })
}

fn spawn_stream_pump<R>(
    reader: Option<R>,
    stream: ExecLogStream,
    exec_id: Uuid,
    logger: ExecLogger,
    tx: mpsc::UnboundedSender<ExecStreamEvent>,
) -> Option<tokio::task::JoinHandle<()>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    if let Some(mut reader) = reader {
        Some(tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        logger.log_chunk(stream.clone(), &buf[..n]).await;
                        let chunk = ExecStreamChunk {
                            exec_id: Some(exec_id),
                            cursor: 0,
                            stream: stream.clone(),
                            data: buf[..n].to_vec(),
                        };
                        if tx.send(ExecStreamEvent::Chunk(chunk)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read {:?} stream: {}", stream, e);
                        break;
                    }
                }
            }
        }))
    } else {
        None
    }
}

fn spawn_wait_for_exit(
    mut child: Child,
    timeout_secs: u64,
    exec_id: Uuid,
    tx: mpsc::UnboundedSender<ExecStreamEvent>,
    pump_handles: Vec<tokio::task::JoinHandle<()>>,
) {
    tokio::spawn(async move {
        let wait_result = if timeout_secs > 0 {
            match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await {
                Ok(res) => res,
                Err(_) => {
                    let _ = child.start_kill();
                    return {
                        let _ = tx.send(ExecStreamEvent::Error {
                            code: "TIMEOUT",
                            message: format!("Command timed out after {} seconds", timeout_secs),
                        });
                    };
                }
            }
        } else {
            child.wait().await
        };

        // Wait for stdout/stderr pumps to finish draining before sending
        // the exit event. Without this, fast commands (e.g. `echo`) can
        // race: child.wait() returns before the pump reads pipe data,
        // causing the Exit event to arrive before Chunk events.
        for handle in pump_handles {
            let _ = handle.await;
        }

        match wait_result {
            Ok(status) => {
                let _ = tx.send(ExecStreamEvent::Exit(ExecStreamExit {
                    exec_id: Some(exec_id),
                    cursor: 0,
                    exit_code: status.code().unwrap_or(-1),
                }));
            }
            Err(e) => {
                let _ = tx.send(ExecStreamEvent::Error {
                    code: "EXEC_ERROR",
                    message: format!("Failed to execute command: {}", e),
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ExecRequest;
    use std::collections::HashMap;

    /// Helper: dispatch a request through an AgentHandler and extract the
    /// response (panics if the handler returns Streaming or an IO error).
    async fn dispatch_response(handler: &AgentHandler, request: AgentRequest) -> AgentResponse {
        let mut buf = Vec::new();
        let mut writer = BufWriter::new(&mut buf);
        match handler.dispatch(request, &mut writer).await {
            Ok(DispatchResult::Response(r)) => r,
            Ok(DispatchResult::Streaming) => panic!("Expected Response, got Streaming"),
            Err(e) => panic!("Dispatch IO error: {e}"),
        }
    }

    // ── Registry tests ──────────────────────────────────────────────

    #[test]
    fn capabilities_does_not_include_unimplemented() {
        let handler = AgentHandler::new();
        let caps = handler.capabilities();
        assert!(
            !caps.contains(&Capability::ConfigureNetwork),
            "ConfigureNetwork is not implemented yet"
        );
    }

    #[test]
    fn capabilities_contains_no_other() {
        let handler = AgentHandler::new();
        for cap in handler.capabilities() {
            assert_ne!(
                cap,
                Capability::Other,
                "must not advertise Capability::Other"
            );
        }
    }

    #[test]
    fn capabilities_includes_all_implemented() {
        let handler = AgentHandler::new();
        assert!(handler.supports(&Capability::Exec));
        assert!(handler.supports(&Capability::ExecStream));
        assert!(handler.supports(&Capability::FileTransfer));
        assert!(handler.supports(&Capability::SshKeyInstall));
        assert!(handler.supports(&Capability::Shutdown));
        assert!(handler.supports(&Capability::TailExecLog));
        assert!(handler.supports(&Capability::AgentUpdate));
    }

    // ── Dispatch tests: protocol-level (no capability needed) ───────

    #[tokio::test]
    async fn dispatch_ping() {
        let handler = AgentHandler::new();
        let response = dispatch_response(&handler, AgentRequest::Ping).await;
        assert!(matches!(response, AgentResponse::Pong));
    }

    #[tokio::test]
    async fn dispatch_ready() {
        let handler = AgentHandler::new();
        let response = dispatch_response(&handler, AgentRequest::Ready).await;
        match response {
            AgentResponse::Ready(r) => {
                assert!(!r.capabilities.is_empty());
                assert!(r.capabilities.contains(&Capability::Exec));
            }
            _ => panic!("Expected Ready response"),
        }
    }

    // ── Dispatch tests: capability-gated handlers ───────────────────

    #[tokio::test]
    async fn dispatch_exec() {
        let handler = AgentHandler::new();
        let request = AgentRequest::Exec(ExecRequest {
            command: vec!["echo".to_string(), "hello".to_string()],
            exec_id: None,
            env: HashMap::new(),
            working_dir: None,
            stdin: None,
            timeout_secs: 0,
        });
        match dispatch_response(&handler, request).await {
            AgentResponse::ExecResult(r) => {
                assert_eq!(r.exit_code, 0);
                assert_eq!(String::from_utf8_lossy(&r.stdout).trim(), "hello");
            }
            other => panic!("Expected ExecResult, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_exec_empty_command() {
        let handler = AgentHandler::new();
        let request = AgentRequest::Exec(ExecRequest {
            command: vec![],
            exec_id: None,
            env: HashMap::new(),
            working_dir: None,
            stdin: None,
            timeout_secs: 0,
        });
        match dispatch_response(&handler, request).await {
            AgentResponse::Error(e) => assert_eq!(e.code, "INVALID_COMMAND"),
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_exec_timeout() {
        let handler = AgentHandler::new();
        let request = AgentRequest::Exec(ExecRequest {
            command: vec!["sleep".to_string(), "10".to_string()],
            exec_id: None,
            env: HashMap::new(),
            working_dir: None,
            stdin: None,
            timeout_secs: 1,
        });
        match dispatch_response(&handler, request).await {
            AgentResponse::Error(e) => assert_eq!(e.code, "TIMEOUT"),
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    // ── Dispatch tests: unregistered capability returns UNSUPPORTED ──

    #[tokio::test]
    async fn dispatch_configure_network_is_unsupported() {
        let handler = AgentHandler::new();
        let request = AgentRequest::ConfigureNetwork(agent_protocol::ConfigureNetworkRequest {
            ip_address: "10.0.0.2/24".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec![],
        });
        match dispatch_response(&handler, request).await {
            AgentResponse::Error(e) => assert_eq!(e.code, "UNSUPPORTED"),
            other => panic!("Expected UNSUPPORTED error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_unknown_request_is_unsupported() {
        let handler = AgentHandler::new();
        let response = dispatch_response(&handler, AgentRequest::Unknown).await;
        match response {
            AgentResponse::Error(e) => assert_eq!(e.code, "UNSUPPORTED"),
            other => panic!("Expected UNSUPPORTED error, got {other:?}"),
        }
    }

    // ── UpdateAgent tests ───────────────────────────────────────────

    /// Spin up a minimal HTTP server that serves `body` at any path.
    /// Returns the base URL (e.g. "http://127.0.0.1:PORT").
    async fn start_http_server(body: Vec<u8>) -> String {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            // Serve one request then shut down.
            if let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                // Read the request (we don't care about its content)
                let _ = stream.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.flush().await;
            }
        });
        format!("http://{addr}")
    }

    /// Spin up a server that returns the given HTTP status code.
    async fn start_http_server_with_status(status: u16) -> String {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {status} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });
        format!("http://{addr}")
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(data))
    }

    #[tokio::test]
    async fn update_agent_success() {
        let fake_binary = b"#!/bin/sh\necho updated\n".to_vec();
        let checksum = sha256_hex(&fake_binary);
        let url = start_http_server(fake_binary.clone()).await;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("chelsea-agent");
        // Create a "current" binary to replace
        tokio::fs::write(&target, b"old binary").await.unwrap();

        let response = perform_agent_update(&format!("{url}/agent"), &checksum, &target).await;
        assert!(
            matches!(response, AgentResponse::Ok),
            "Expected Ok, got {response:?}"
        );

        // Verify the file was replaced with the new content
        let contents = tokio::fs::read(&target).await.unwrap();
        assert_eq!(contents, fake_binary);

        // Verify executable permission
        use std::os::unix::fs::PermissionsExt;
        let perms = tokio::fs::metadata(&target).await.unwrap().permissions();
        assert_eq!(perms.mode() & 0o755, 0o755);
    }

    #[tokio::test]
    async fn update_agent_checksum_mismatch() {
        let fake_binary = b"some binary content".to_vec();
        let url = start_http_server(fake_binary).await;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("chelsea-agent");
        tokio::fs::write(&target, b"old binary").await.unwrap();

        let response =
            perform_agent_update(&format!("{url}/agent"), "0000000000000000", &target).await;
        match response {
            AgentResponse::Error(e) => {
                assert_eq!(e.code, "CHECKSUM_MISMATCH");
                assert!(e.message.contains("SHA-256 mismatch"));
            }
            other => panic!("Expected CHECKSUM_MISMATCH, got {other:?}"),
        }

        // Original binary should be untouched
        let contents = tokio::fs::read(&target).await.unwrap();
        assert_eq!(contents, b"old binary");
    }

    #[tokio::test]
    async fn update_agent_download_404() {
        let url = start_http_server_with_status(404).await;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("chelsea-agent");
        tokio::fs::write(&target, b"old binary").await.unwrap();

        let response = perform_agent_update(&format!("{url}/agent"), "irrelevant", &target).await;
        match response {
            AgentResponse::Error(e) => {
                assert_eq!(e.code, "DOWNLOAD_ERROR");
                assert!(e.message.contains("404"));
            }
            other => panic!("Expected DOWNLOAD_ERROR, got {other:?}"),
        }

        // Original binary should be untouched
        let contents = tokio::fs::read(&target).await.unwrap();
        assert_eq!(contents, b"old binary");
    }

    #[tokio::test]
    async fn update_agent_unreachable_url() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("chelsea-agent");
        tokio::fs::write(&target, b"old binary").await.unwrap();

        // Port 1 is almost certainly not listening
        let response =
            perform_agent_update("http://127.0.0.1:1/agent", "irrelevant", &target).await;
        match response {
            AgentResponse::Error(e) => assert_eq!(e.code, "DOWNLOAD_ERROR"),
            other => panic!("Expected DOWNLOAD_ERROR, got {other:?}"),
        }
    }
}
async fn stream_session<W>(
    session: Arc<ExecStreamSession>,
    cursor: Option<u64>,
    writer: &mut BufWriter<W>,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut last_cursor = cursor;
    let subscription = match session.subscribe(cursor).await {
        Ok(sub) => sub,
        Err(SessionError::CursorTooOld {
            requested,
            available,
        }) => {
            send_agent_response(
                writer,
                AgentResponse::error(
                    "CURSOR_TOO_OLD",
                    format!(
                        "Requested cursor {} is older than available backlog starting at {}",
                        requested, available
                    ),
                ),
            )
            .await?;
            return Ok(());
        }
        Err(SessionError::NotFound(missing)) => {
            send_agent_response(
                writer,
                AgentResponse::error(
                    "EXEC_STREAM_NOT_FOUND",
                    format!("Exec stream {} is no longer active", missing),
                ),
            )
            .await?;
            return Ok(());
        }
        Err(err) => {
            send_agent_response(
                writer,
                AgentResponse::error("STREAM_ERROR", format!("{err:?}")),
            )
            .await?;
            return Ok(());
        }
    };

    for event in subscription.backlog {
        send_agent_response(writer, event.response.clone()).await?;
        last_cursor = Some(event.cursor);
        if event.terminal {
            return Ok(());
        }
    }

    let mut receiver = subscription.receiver;

    loop {
        match receiver.recv().await {
            Ok(event) => {
                send_agent_response(writer, event.response.clone()).await?;
                last_cursor = Some(event.cursor);
                if event.terminal {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                match session.collect_backlog(last_cursor).await {
                    Ok(events) => {
                        if events.is_empty() {
                            continue;
                        }
                        for event in events {
                            send_agent_response(writer, event.response.clone()).await?;
                            last_cursor = Some(event.cursor);
                            if event.terminal {
                                return Ok(());
                            }
                        }
                    }
                    Err(SessionError::CursorTooOld {
                        requested,
                        available,
                    }) => {
                        send_agent_response(
                            writer,
                            AgentResponse::error(
                                "CURSOR_TOO_OLD",
                                format!(
                                    "Requested cursor {} is older than available backlog starting at {}",
                                    requested, available
                                ),
                            ),
                        )
                        .await?;
                        return Ok(());
                    }
                    Err(err) => {
                        send_agent_response(
                            writer,
                            AgentResponse::error("STREAM_ERROR", format!("{err:?}")),
                        )
                        .await?;
                        return Ok(());
                    }
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }

    Ok(())
}
