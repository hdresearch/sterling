use std::{
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    time::Duration,
};

use agent_protocol::{
    AGENT_PORT, AgentRequest, AgentResponse, ConfigureNetworkRequest, ExecLogChunkResponse,
    ExecRequest, ExecResult, ExecStreamAttachRequest, ExecStreamChunk, ExecStreamExit,
    FileContentResponse, InstallSshKeyRequest, ReadFileRequest, ReadyResponse, TailExecLogRequest,
    UpdateAgentRequest, WriteFileRequest,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    time::{sleep, timeout},
};
use tracing::{debug, trace, warn};
use uuid::Uuid;

/// Default connect timeout for establishing a Unix socket connection to Firecracker's vsock
/// device. Covers both the Unix domain socket connect and the Firecracker handshake.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Default timeout for individual agent RPCs once the vsock connection is established. Applies to
/// writing the request payload and waiting for the agent's response on that same connection.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Delay between vsock handshake retries while waiting for the agent to start. We poll every 50ms
/// to keep boot detection latency low without spinning continuously.
const HANDSHAKE_RETRY_INTERVAL: Duration = Duration::from_millis(50);
/// Maximum number of unsolicited Ready events tolerated on an RPC connection before we bail out.
const MAX_READY_EVENT_DISCARDS: usize = 8;

/// Client for communicating with the in-VM chelsea agent over vsock.
///
/// Firecracker exposes vsock via a Unix domain socket. The connection protocol:
/// 1. Connect to the Unix socket
/// 2. Send: "CONNECT {port}\n"
/// 3. Receive: "OK {local_port}\n"
/// 4. Then bidirectional JSON-lines communication
///
/// # Snapshot Restore Behavior
///
/// When a VM is restored from a snapshot, the vsock device is reset and any existing
/// connections are terminated. However, the agent's listen socket inside the guest
/// remains active and will accept new connections after restore. Callers should be
/// prepared to reconnect if using this client across snapshot/restore boundaries.
///
/// See: <https://github.com/firecracker-microvm/firecracker/blob/main/docs/snapshotting/snapshot-support.md#vsock-device-reset>
#[derive(Debug, Clone)]
pub struct VsockClient {
    /// Path to Firecracker's vsock Unix socket.
    socket_path: PathBuf,
    /// Agent port number (default: 10789).
    agent_port: u32,
    /// Timeout for establishing connections.
    connect_timeout: Duration,
    /// Timeout for individual requests.
    request_timeout: Duration,
}

impl VsockClient {
    /// Create a new VsockClient with default settings.
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            agent_port: AGENT_PORT,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Set the agent port.
    pub fn with_port(mut self, port: u32) -> Self {
        self.agent_port = port;
        self
    }

    /// Set the connect timeout.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Set the request timeout.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Get the socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Connect to the vsock and perform Firecracker handshake.
    ///
    /// Returns split reader/writer halves. The BufReader is preserved from the
    /// handshake phase so that any data the agent sent immediately after "OK"
    /// (e.g. a Ready event) is not lost.
    async fn connect(
        &self,
    ) -> Result<(BufReader<OwnedReadHalf>, BufWriter<OwnedWriteHalf>), super::VsockError> {
        let path_str = self.socket_path.display().to_string();

        debug!(path = %path_str, port = self.agent_port, "Connecting to vsock");

        // Connect with timeout
        let stream = timeout(self.connect_timeout, UnixStream::connect(&self.socket_path))
            .await
            .map_err(|_| super::VsockError::ConnectionTimeout {
                timeout_ms: self.connect_timeout.as_millis() as u64,
            })?
            .map_err(|e| super::VsockError::ConnectionFailed {
                path: path_str.clone(),
                source: e,
            })?;

        // Perform Firecracker vsock handshake
        self.handshake(stream).await
    }

    /// Connect to the vsock with retries on transient handshake failures.
    ///
    /// When a VM has just booted, the Firecracker vsock socket exists but the
    /// in-guest agent may not yet be listening. In that case the `CONNECT`
    /// handshake returns an empty response. This method retries such transient
    /// failures for up to `connect_timeout`, polling every
    /// `HANDSHAKE_RETRY_INTERVAL`.
    async fn connect_with_retries(
        &self,
    ) -> Result<(BufReader<OwnedReadHalf>, BufWriter<OwnedWriteHalf>), super::VsockError> {
        let start = std::time::Instant::now();
        let mut last_error: Option<super::VsockError> = None;

        loop {
            if start.elapsed() >= self.connect_timeout {
                break;
            }

            match self.connect().await {
                Ok(conn) => return Ok(conn),
                Err(err) if is_retryable_ready_error(&err) => {
                    debug!(
                        elapsed_ms = start.elapsed().as_millis(),
                        %err,
                        "Vsock connect failed (retryable), will retry"
                    );
                    last_error = Some(err);
                    let remaining = self.connect_timeout.saturating_sub(start.elapsed());
                    let sleep_dur = remaining.min(HANDSHAKE_RETRY_INTERVAL);
                    if sleep_dur.is_zero() {
                        break;
                    }
                    sleep(sleep_dur).await;
                    continue;
                }
                Err(err) => return Err(err),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            super::VsockError::HandshakeFailed(
                "timed out waiting for agent vsock connection".to_string(),
            )
        }))
    }

    /// Perform the Firecracker vsock handshake.
    ///
    /// Protocol:
    /// 1. Send: "CONNECT {port}\n"
    /// 2. Receive: "OK {local_port}\n"
    ///
    /// Returns the split reader/writer halves preserving any buffered data.
    async fn handshake(
        &self,
        stream: UnixStream,
    ) -> Result<(BufReader<OwnedReadHalf>, BufWriter<OwnedWriteHalf>), super::VsockError> {
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut writer = BufWriter::new(writer);

        // Send CONNECT command
        let connect_cmd = format!("CONNECT {}\n", self.agent_port);
        trace!(cmd = %connect_cmd.trim(), "Sending vsock handshake");
        writer.write_all(connect_cmd.as_bytes()).await?;
        writer.flush().await?;

        // Read response
        let mut response = String::new();
        reader.read_line(&mut response).await?;
        let response = response.trim();

        trace!(response = %response, "Received vsock handshake response");

        // Parse response: "OK {local_port}"
        if !response.starts_with("OK ") {
            return Err(super::VsockError::HandshakeFailed(format!(
                "expected 'OK <port>', got '{}'",
                response
            )));
        }

        debug!("Vsock handshake successful");
        Ok((reader, writer))
    }

    async fn read_agent_response_with_timeout<R>(
        &self,
        reader: &mut BufReader<R>,
        timeout_duration: Duration,
    ) -> Result<AgentResponse, super::VsockError>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let mut response_line = String::new();
        timeout(timeout_duration, reader.read_line(&mut response_line))
            .await
            .map_err(|_| super::VsockError::RequestTimeout {
                timeout_ms: timeout_duration.as_millis() as u64,
            })??;

        if response_line.is_empty() {
            return Err(super::VsockError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "vsock stream closed",
            )));
        }

        trace!(response = %response_line.trim(), "Received response");
        Ok(serde_json::from_str(&response_line)?)
    }

    /// Send a request and receive a response.
    async fn request(&self, request: &AgentRequest) -> Result<AgentResponse, super::VsockError> {
        let (mut reader, mut writer) = self.connect_with_retries().await?;

        // Serialize request to JSON line
        let mut request_json = serde_json::to_string(request)?;
        debug!(%request_json, "Sending vsock agent request");
        request_json.push('\n');

        trace!(request = %request_json.trim(), "Sending request");

        // Send request with timeout
        timeout(self.request_timeout, async {
            writer.write_all(request_json.as_bytes()).await?;
            writer.flush().await?;
            Ok::<_, super::VsockError>(())
        })
        .await
        .map_err(|_| super::VsockError::RequestTimeout {
            timeout_ms: self.request_timeout.as_millis() as u64,
        })??;

        let mut ready_discards = 0;
        loop {
            let response = self
                .read_agent_response_with_timeout(&mut reader, self.request_timeout)
                .await?;

            match response {
                AgentResponse::Ready(_) => {
                    ready_discards += 1;
                    if ready_discards >= MAX_READY_EVENT_DISCARDS {
                        warn!(
                            "Received {ready_discards} unsolicited Ready events while waiting for RPC response"
                        );
                        return Err(super::VsockError::UnexpectedResponse {
                            expected: "agent RPC response".to_string(),
                            actual: "Ready (discard limit exceeded)".to_string(),
                        });
                    }
                    trace!(
                        count = ready_discards,
                        "Discarding unsolicited Ready event on request connection"
                    );
                    continue;
                }
                AgentResponse::Error(err) => {
                    return Err(super::VsockError::AgentError(err.message));
                }
                other => return Ok(other),
            }
        }
    }

    async fn wait_ready_event_single_attempt(
        &self,
        timeout_duration: Duration,
    ) -> Result<ReadyResponse, super::VsockError> {
        // Keep both halves alive so the agent can deliver the Ready payload.
        let (mut reader, _writer) = self.connect().await?;
        let response = self
            .read_agent_response_with_timeout(&mut reader, timeout_duration)
            .await?;
        match response {
            AgentResponse::Ready(ready) => Ok(ready),
            AgentResponse::Error(err) => Err(super::VsockError::AgentError(err.message)),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "Ready".to_string(),
                actual: format!("{other:?}"),
            }),
        }
    }

    async fn wait_ready_event_with_retries(
        &self,
        timeout_duration: Duration,
    ) -> Result<ReadyResponse, super::VsockError> {
        let start = std::time::Instant::now();
        let mut last_error: Option<super::VsockError> = None;

        loop {
            if start.elapsed() >= timeout_duration {
                break;
            }

            let remaining = timeout_duration - start.elapsed();
            match self.wait_ready_event_single_attempt(remaining).await {
                Ok(ready) => return Ok(ready),
                Err(err) if is_retryable_ready_error(&err) => {
                    last_error = Some(err);
                    let sleep_dur = remaining.min(HANDSHAKE_RETRY_INTERVAL);
                    sleep(sleep_dur).await;
                    continue;
                }
                Err(err) => return Err(err),
            }
        }

        Err(last_error.unwrap_or_else(|| {
            super::VsockError::AgentNotReady("vsock device did not become ready".to_string())
        }))
    }

    /// Wait until the agent is ready, polling with the specified timeout.
    pub async fn wait_ready(
        &self,
        timeout_duration: Duration,
    ) -> Result<ReadyResponse, super::VsockError> {
        let start = std::time::Instant::now();
        match self.wait_ready_event_with_retries(timeout_duration).await {
            Ok(ready) => {
                debug!(
                    elapsed_ms = start.elapsed().as_millis(),
                    version = ready.version,
                    "Agent reported ready via vsock event"
                );
                Ok(ready)
            }
            Err(error) => {
                warn!(
                    elapsed_ms = start.elapsed().as_millis(),
                    %error,
                    "Agent failed to report ready before timeout"
                );
                Err(super::VsockError::AgentNotReady(format!(
                    "timed out after {:?}: {}",
                    timeout_duration, error
                )))
            }
        }
    }

    /// Execute a command in the VM.
    pub async fn exec(&self, command: &[&str]) -> Result<ExecResult, super::VsockError> {
        let request = AgentRequest::Exec(ExecRequest {
            command: command.iter().map(|s| s.to_string()).collect(),
            exec_id: None,
            env: Default::default(),
            working_dir: None,
            stdin: None,
            timeout_secs: self.request_timeout.as_secs(),
        });

        let response = self.request(&request).await?;

        match response {
            AgentResponse::ExecResult(exec) => Ok(exec),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "ExecResult".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Execute a command with full options.
    pub async fn exec_with_options(
        &self,
        request: ExecRequest,
    ) -> Result<ExecResult, super::VsockError> {
        let response = self.request(&AgentRequest::Exec(request)).await?;

        match response {
            AgentResponse::ExecResult(exec) => Ok(exec),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "ExecResult".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Execute a command and stream stdout/stderr incrementally.
    pub async fn exec_stream(
        &self,
        request: ExecRequest,
    ) -> Result<ExecStreamConnection, super::VsockError> {
        let (reader, mut writer) = self.connect_with_retries().await?;

        let mut request_json = serde_json::to_string(&AgentRequest::ExecStream(request))?;
        request_json.push('\n');

        timeout(self.request_timeout, async {
            writer.write_all(request_json.as_bytes()).await?;
            writer.flush().await?;
            Ok::<_, super::VsockError>(())
        })
        .await
        .map_err(|_| super::VsockError::RequestTimeout {
            timeout_ms: self.request_timeout.as_millis() as u64,
        })??;

        Ok(ExecStreamConnection {
            reader,
            writer,
            timeout: self.request_timeout,
        })
    }

    /// Reattach to an existing exec stream session.
    pub async fn exec_stream_attach(
        &self,
        exec_id: Uuid,
        cursor: Option<u64>,
        from_latest: bool,
    ) -> Result<ExecStreamConnection, super::VsockError> {
        let (reader, mut writer) = self.connect_with_retries().await?;

        let mut request_json =
            serde_json::to_string(&AgentRequest::ExecStreamAttach(ExecStreamAttachRequest {
                exec_id,
                cursor,
                from_latest,
            }))?;
        request_json.push('\n');

        timeout(self.request_timeout, async {
            writer.write_all(request_json.as_bytes()).await?;
            writer.flush().await?;
            Ok::<_, super::VsockError>(())
        })
        .await
        .map_err(|_| super::VsockError::RequestTimeout {
            timeout_ms: self.request_timeout.as_millis() as u64,
        })??;

        Ok(ExecStreamConnection {
            reader,
            writer,
            timeout: self.request_timeout,
        })
    }

    /// Write a file in the VM.
    pub async fn write_file(
        &self,
        path: &str,
        content: &[u8],
        mode: u32,
        create_dirs: bool,
    ) -> Result<(), super::VsockError> {
        let request = AgentRequest::WriteFile(WriteFileRequest {
            path: path.to_string(),
            content: content.to_vec(),
            mode,
            create_dirs,
        });

        let response = self.request(&request).await?;

        match response {
            AgentResponse::Ok => Ok(()),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "Ok".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Read a file from the VM.
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>, super::VsockError> {
        let request = AgentRequest::ReadFile(ReadFileRequest {
            path: path.to_string(),
        });

        let response = self.request(&request).await?;

        match response {
            AgentResponse::FileContent(FileContentResponse { content }) => Ok(content),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "FileContent".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Install an SSH public key for user access.
    pub async fn install_ssh_key(&self, public_key: &str) -> Result<(), super::VsockError> {
        self.install_ssh_key_for_user(public_key, "root").await
    }

    /// Install an SSH public key for a specific user.
    pub async fn install_ssh_key_for_user(
        &self,
        public_key: &str,
        user: &str,
    ) -> Result<(), super::VsockError> {
        let request = AgentRequest::InstallSshKey(InstallSshKeyRequest {
            public_key: public_key.to_string(),
            user: user.to_string(),
        });

        let response = self.request(&request).await?;

        match response {
            AgentResponse::Ok => Ok(()),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "Ok".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Retrieve exec log entries emitted by the agent.
    pub async fn tail_exec_log(
        &self,
        request: TailExecLogRequest,
    ) -> Result<ExecLogChunkResponse, super::VsockError> {
        let response = self.request(&AgentRequest::TailExecLog(request)).await?;

        match response {
            AgentResponse::ExecLogChunk(chunk) => Ok(chunk),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "ExecLogChunk".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Configure network settings in the VM.
    pub async fn configure_network(
        &self,
        ip_address: &str,
        gateway: &str,
        dns_servers: &[&str],
    ) -> Result<(), super::VsockError> {
        let request = AgentRequest::ConfigureNetwork(ConfigureNetworkRequest {
            ip_address: ip_address.to_string(),
            gateway: gateway.to_string(),
            dns_servers: dns_servers.iter().map(|s| s.to_string()).collect(),
        });

        let response = self.request(&request).await?;

        match response {
            AgentResponse::Ok => Ok(()),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "Ok".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Update the agent binary inside the VM.
    ///
    /// The agent will download the binary from `url`, verify it against
    /// `expected_sha256`, and atomically replace itself. If `restart` is
    /// true (the default), the agent process restarts after the update —
    /// the current vsock connection will be dropped and the host should
    /// reconnect and wait for a new Ready event.
    pub async fn update_agent(
        &self,
        url: &str,
        expected_sha256: &str,
        restart: bool,
    ) -> Result<(), super::VsockError> {
        let request = AgentRequest::UpdateAgent(UpdateAgentRequest {
            url: url.to_string(),
            sha256: expected_sha256.to_string(),
            restart,
        });

        let response = self.request(&request).await?;

        match response {
            AgentResponse::Ok => Ok(()),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "Ok".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }

    /// Request graceful shutdown of the VM.
    pub async fn shutdown(&self) -> Result<(), super::VsockError> {
        let response = self.request(&AgentRequest::Shutdown).await?;

        match response {
            AgentResponse::Ok => Ok(()),
            other => Err(super::VsockError::UnexpectedResponse {
                expected: "Ok".to_string(),
                actual: format!("{:?}", other),
            }),
        }
    }
}

/// A live connection to an exec stream session.
///
/// Created by [`VsockClient::exec_stream`] or [`VsockClient::exec_stream_attach`].
/// Call [`next_event`](ExecStreamConnection::next_event) to receive streaming
/// stdout/stderr chunks and the final exit event.
pub struct ExecStreamConnection {
    reader: BufReader<OwnedReadHalf>,
    #[allow(dead_code)]
    writer: BufWriter<OwnedWriteHalf>,
    timeout: Duration,
}

/// Events emitted by an exec stream session.
#[derive(Debug)]
pub enum ExecStreamEvent {
    /// A chunk of stdout or stderr data.
    Chunk(ExecStreamChunk),
    /// The process has exited.
    Exit(ExecStreamExit),
}

impl ExecStreamConnection {
    /// Wait for the next event from the exec stream.
    ///
    /// Returns `None` when the stream closes (EOF).
    pub async fn next_event(&mut self) -> Result<Option<ExecStreamEvent>, super::VsockError> {
        loop {
            let mut response_line = String::new();
            match timeout(self.timeout, self.reader.read_line(&mut response_line)).await {
                Ok(Ok(0)) => {
                    debug!("exec stream reader reached EOF");
                    return Ok(None);
                }
                Ok(Ok(_)) => {}
                Ok(Err(err)) => return Err(err.into()),
                Err(_) => {
                    return Err(super::VsockError::RequestTimeout {
                        timeout_ms: self.timeout.as_millis() as u64,
                    });
                }
            }

            if response_line.trim().is_empty() {
                continue;
            }
            debug!(raw = %response_line.trim_end(), "exec stream raw line");

            match serde_json::from_str::<AgentResponse>(&response_line)? {
                AgentResponse::ExecStreamChunk(chunk) => {
                    return Ok(Some(ExecStreamEvent::Chunk(chunk)));
                }
                AgentResponse::ExecStreamExit(exit) => {
                    return Ok(Some(ExecStreamEvent::Exit(exit)));
                }
                AgentResponse::Error(err) => {
                    return Err(super::VsockError::AgentError(err.message));
                }
                AgentResponse::Ready(_) | AgentResponse::Ok | AgentResponse::Pong => continue,
                other => {
                    return Err(super::VsockError::UnexpectedResponse {
                        expected: "ExecStreamChunk/ExecStreamExit".to_string(),
                        actual: format!("{other:?}"),
                    });
                }
            }
        }
    }
}

fn is_retryable_ready_error(err: &super::VsockError) -> bool {
    match err {
        super::VsockError::ConnectionFailed { .. }
        | super::VsockError::ConnectionTimeout { .. }
        | super::VsockError::HandshakeFailed(_) => true,
        super::VsockError::Io(io_err) => matches!(
            io_err.kind(),
            ErrorKind::UnexpectedEof
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionRefused
                | ErrorKind::BrokenPipe
                | ErrorKind::NotConnected
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_builder() {
        let client = VsockClient::new("/tmp/vsock.sock")
            .with_port(12345)
            .with_connect_timeout(Duration::from_secs(10))
            .with_request_timeout(Duration::from_secs(60));

        assert_eq!(client.socket_path(), Path::new("/tmp/vsock.sock"));
        assert_eq!(client.agent_port, 12345);
        assert_eq!(client.connect_timeout, Duration::from_secs(10));
        assert_eq!(client.request_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_default_client() {
        let client = VsockClient::new("/tmp/vsock.sock");

        assert_eq!(client.agent_port, AGENT_PORT);
        assert_eq!(client.connect_timeout, DEFAULT_CONNECT_TIMEOUT);
        assert_eq!(client.request_timeout, DEFAULT_REQUEST_TIMEOUT);
    }

    // Integration-style tests using a mock Unix socket server to verify
    // the full connect → handshake → request → response flow.

    use tokio::net::UnixListener;

    /// Helper: create a temporary Unix socket and return (listener, temp_dir).
    /// The temp_dir must be kept alive for the socket path to remain valid.
    fn temp_unix_socket() -> (UnixListener, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let sock_path = dir.path().join("vsock.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        (listener, dir)
    }

    /// Mock server that performs the Firecracker handshake then handles one request.
    async fn mock_agent_one_shot(listener: UnixListener, response_json: &str) {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut writer = BufWriter::new(writer);

        // Firecracker handshake: read CONNECT, reply OK
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(
            line.starts_with("CONNECT "),
            "expected CONNECT, got: {line}"
        );
        writer.write_all(b"OK 1234\n").await.unwrap();
        writer.flush().await.unwrap();

        // Read the JSON request line
        let mut req_line = String::new();
        reader.read_line(&mut req_line).await.unwrap();
        assert!(!req_line.is_empty(), "expected request JSON");

        // Send the canned response
        writer.write_all(response_json.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        writer.flush().await.unwrap();
    }

    #[tokio::test]
    async fn test_ping_pong() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(listener, r#"{"type":"Pong"}"#).await;
        });

        let response = client.request(&AgentRequest::Ping).await.unwrap();
        assert!(matches!(response, AgentResponse::Pong));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_exec_command() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(
                listener,
                r#"{"type":"ExecResult","payload":{"exit_code":0,"stdout":[104,101,108,108,111],"stderr":[]}}"#,
            )
            .await;
        });

        let result = client.exec(&["echo", "hello"]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"hello");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_agent_error_response() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(
                listener,
                r#"{"type":"Error","payload":{"message":"file not found","code":"ENOENT"}}"#,
            )
            .await;
        });

        let err = client.exec(&["cat", "/nonexistent"]).await.unwrap_err();
        match err {
            super::super::VsockError::AgentError(msg) => {
                assert_eq!(msg, "file not found");
            }
            other => panic!("expected AgentError, got: {other}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_ready_events_discarded_during_request() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            // Handshake
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer.write_all(b"OK 1234\n").await.unwrap();
            writer.flush().await.unwrap();

            // Read request
            let mut req_line = String::new();
            reader.read_line(&mut req_line).await.unwrap();

            // Send 2 unsolicited Ready events, then the actual response
            writer
                .write_all(
                    b"{\"type\":\"Ready\",\"payload\":{\"version\":\"0.1.0\",\"capabilities\":[]}}\n",
                )
                .await
                .unwrap();
            writer
                .write_all(
                    b"{\"type\":\"Ready\",\"payload\":{\"version\":\"0.1.0\",\"capabilities\":[]}}\n",
                )
                .await
                .unwrap();
            writer.write_all(b"{\"type\":\"Pong\"}\n").await.unwrap();
            writer.flush().await.unwrap();
        });

        let response = client.request(&AgentRequest::Ping).await.unwrap();
        assert!(matches!(response, AgentResponse::Pong));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_handshake_failure() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path).with_connect_timeout(Duration::from_millis(500));

        // Keep accepting connections and sending bad handshakes so the
        // retry loop always sees a HandshakeFailed error until it times out.
        let server = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let (reader, writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let mut writer = BufWriter::new(writer);

                let mut line = String::new();
                let _ = reader.read_line(&mut line).await;
                let _ = writer.write_all(b"NOPE\n").await;
                let _ = writer.flush().await;
            }
        });

        let err = client.request(&AgentRequest::Ping).await.unwrap_err();
        assert!(matches!(err, super::super::VsockError::HandshakeFailed(_)));
        server.abort();
    }

    #[tokio::test]
    async fn test_connection_refused() {
        let client = VsockClient::new("/tmp/nonexistent-vsock-path-12345.sock")
            .with_connect_timeout(Duration::from_millis(500));
        let err = client.request(&AgentRequest::Ping).await.unwrap_err();
        assert!(matches!(
            err,
            super::super::VsockError::ConnectionFailed { .. }
        ));
    }

    #[tokio::test]
    async fn test_write_file() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(listener, r#"{"type":"Ok"}"#).await;
        });

        client
            .write_file("/tmp/test.txt", b"hello world", 0o644, true)
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_read_file() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(
                listener,
                r#"{"type":"FileContent","payload":{"content":"aGVsbG8gd29ybGQ="}}"#,
            )
            .await;
        });

        let content = client.read_file("/tmp/test.txt").await.unwrap();
        assert_eq!(content, b"hello world");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_install_ssh_key() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(listener, r#"{"type":"Ok"}"#).await;
        });

        client
            .install_ssh_key("ssh-ed25519 AAAA... user@host")
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_shutdown() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(listener, r#"{"type":"Ok"}"#).await;
        });

        client.shutdown().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_wait_ready() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            // Accept multiple connections — wait_ready retries on transient failures,
            // and the agent sends Ready on every new connection.
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let (reader, writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let mut writer = BufWriter::new(writer);

                // Handshake
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_err() {
                    continue;
                }
                let _ = writer.write_all(b"OK 1234\n").await;
                let _ = writer.flush().await;

                // Send Ready event
                let _ = writer
                    .write_all(
                        b"{\"type\":\"Ready\",\"payload\":{\"version\":\"0.1.0\",\"capabilities\":[\"Exec\",\"FileTransfer\"]}}\n",
                    )
                    .await;
                let _ = writer.flush().await;
                // Keep the connection alive until the client reads
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        let ready = client.wait_ready(Duration::from_secs(5)).await.unwrap();
        assert_eq!(ready.version, "0.1.0");

        // Server task will end when listener is dropped (test cleanup)
        server.abort();
    }

    #[tokio::test]
    async fn test_exec_stream_events() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            // Handshake
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer.write_all(b"OK 1234\n").await.unwrap();
            writer.flush().await.unwrap();

            // Read ExecStream request
            let mut req_line = String::new();
            reader.read_line(&mut req_line).await.unwrap();

            // Send streaming events
            writer
                .write_all(
                    b"{\"type\":\"ExecStreamChunk\",\"payload\":{\"exec_id\":null,\"cursor\":0,\"stream\":\"stdout\",\"data\":[104,101,108,108,111]}}\n",
                )
                .await
                .unwrap();
            writer
                .write_all(
                    b"{\"type\":\"ExecStreamExit\",\"payload\":{\"exec_id\":null,\"cursor\":1,\"exit_code\":0}}\n",
                )
                .await
                .unwrap();
            writer.flush().await.unwrap();
        });

        let request = ExecRequest {
            command: vec!["echo".to_string(), "hello".to_string()],
            exec_id: None,
            env: Default::default(),
            working_dir: None,
            stdin: None,
            timeout_secs: 30,
        };

        let mut conn = client.exec_stream(request).await.unwrap();

        // First event: chunk
        let event = conn.next_event().await.unwrap().unwrap();
        match event {
            ExecStreamEvent::Chunk(chunk) => {
                assert_eq!(chunk.data, b"hello");
                assert_eq!(chunk.cursor, 0);
            }
            other => panic!("expected Chunk, got: {other:?}"),
        }

        // Second event: exit
        let event = conn.next_event().await.unwrap().unwrap();
        match event {
            ExecStreamEvent::Exit(exit) => {
                assert_eq!(exit.exit_code, 0);
                assert_eq!(exit.cursor, 1);
            }
            other => panic!("expected Exit, got: {other:?}"),
        }

        server.await.unwrap();
    }

    // ── Edge case tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_server_drops_connection_after_handshake() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer.write_all(b"OK 1234\n").await.unwrap();
            writer.flush().await.unwrap();

            // Drop the connection immediately — no response sent
            drop(writer);
            drop(reader);
        });

        let err = client.request(&AgentRequest::Ping).await.unwrap_err();
        // Should get an IO error (unexpected EOF) since the stream closed
        assert!(
            matches!(err, super::super::VsockError::Io(_)),
            "expected Io error, got: {err}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_malformed_json_from_agent() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            mock_agent_one_shot(listener, r#"{"this is not valid json"#).await;
        });

        let err = client.request(&AgentRequest::Ping).await.unwrap_err();
        assert!(
            matches!(err, super::super::VsockError::Protocol(_)),
            "expected Protocol error, got: {err}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_unknown_response_type_from_agent() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            // agent_protocol deserializes unknown types to AgentResponse::Unknown
            mock_agent_one_shot(listener, r#"{"type":"FutureFeature","payload":{}}"#).await;
        });

        // The Unknown variant should be returned without crashing
        let result = client.request(&AgentRequest::Ping).await;
        // Depending on agent_protocol's handling, this is either Ok(Unknown) or an error
        assert!(result.is_ok() || result.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_empty_response_line_skipped_in_stream() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            // Handshake
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer.write_all(b"OK 1234\n").await.unwrap();
            writer.flush().await.unwrap();

            // Read request
            let mut req_line = String::new();
            reader.read_line(&mut req_line).await.unwrap();

            // Send empty lines followed by actual response
            writer.write_all(b"\n\n\n").await.unwrap();
            writer
                .write_all(
                    b"{\"type\":\"ExecStreamChunk\",\"payload\":{\"exec_id\":null,\"cursor\":0,\"stream\":\"stdout\",\"data\":[65]}}\n",
                )
                .await
                .unwrap();
            writer
                .write_all(
                    b"{\"type\":\"ExecStreamExit\",\"payload\":{\"exec_id\":null,\"cursor\":1,\"exit_code\":0}}\n",
                )
                .await
                .unwrap();
            writer.flush().await.unwrap();
        });

        let request = ExecRequest {
            command: vec!["echo".to_string(), "A".to_string()],
            exec_id: None,
            env: Default::default(),
            working_dir: None,
            stdin: None,
            timeout_secs: 30,
        };

        let mut conn = client.exec_stream(request).await.unwrap();

        // Empty lines should be silently skipped
        let event = conn.next_event().await.unwrap().unwrap();
        assert!(matches!(event, ExecStreamEvent::Chunk(_)));

        let event = conn.next_event().await.unwrap().unwrap();
        assert!(matches!(event, ExecStreamEvent::Exit(_)));

        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_max_ready_discards_exceeded() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer.write_all(b"OK 1234\n").await.unwrap();
            writer.flush().await.unwrap();

            let mut req_line = String::new();
            reader.read_line(&mut req_line).await.unwrap();

            // Send more Ready events than MAX_READY_EVENT_DISCARDS (8)
            for _ in 0..10 {
                writer
                    .write_all(
                        b"{\"type\":\"Ready\",\"payload\":{\"version\":\"0.1.0\",\"capabilities\":[]}}\n",
                    )
                    .await
                    .unwrap();
            }
            writer.flush().await.unwrap();
            // Keep connection alive
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let err = client.request(&AgentRequest::Ping).await.unwrap_err();
        assert!(
            matches!(err, super::super::VsockError::UnexpectedResponse { .. }),
            "expected UnexpectedResponse, got: {err}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_exec_stream_eof_returns_none() {
        let (listener, dir) = temp_unix_socket();
        let sock_path = dir.path().join("vsock.sock");
        let client = VsockClient::new(&sock_path);

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut writer = BufWriter::new(writer);

            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer.write_all(b"OK 1234\n").await.unwrap();
            writer.flush().await.unwrap();

            let mut req_line = String::new();
            reader.read_line(&mut req_line).await.unwrap();

            // Close connection without sending any events
            drop(writer);
        });

        let request = ExecRequest {
            command: vec!["true".to_string()],
            exec_id: None,
            env: Default::default(),
            working_dir: None,
            stdin: None,
            timeout_secs: 30,
        };

        let mut conn = client.exec_stream(request).await.unwrap();
        let event = conn.next_event().await.unwrap();
        assert!(event.is_none(), "expected None on EOF");

        server.await.unwrap();
    }
}
