//! Chelsea Agent - In-VM management agent for Firecracker VMs.
//!
//! This agent runs inside Firecracker VMs and listens on vsock for management
//! commands from the Chelsea host. It provides a simple JSON-over-newline
//! protocol for executing commands, managing files, and handling VM lifecycle.

mod error;
mod exec_stream;
pub mod handlers;
mod protocol;

use crate::handlers::{AgentHandler, DispatchResult};
use crate::protocol::{AgentRequest, AgentResponse};
use std::env;
use std::io;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

#[cfg(target_os = "linux")]
use tokio_vsock::{VMADDR_CID_ANY, VMADDR_CID_HOST, VsockAddr, VsockListener};

/// Default vsock port for the agent.
const DEFAULT_PORT: u32 = 10789;

/// Maximum number of concurrent connections the agent will accept.
/// The only legitimate client is the Chelsea host, so this is kept low.
const MAX_CONCURRENT_CONNECTIONS: usize = 16;

/// Parse command line arguments.
fn parse_args() -> (u32, bool) {
    let args: Vec<String> = env::args().collect();
    let mut port = DEFAULT_PORT;
    let mut use_tcp = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or_else(|_| {
                        eprintln!("Invalid port number: {}", args[i + 1]);
                        std::process::exit(1);
                    });
                    i += 2;
                } else {
                    eprintln!("Missing port number after --port");
                    std::process::exit(1);
                }
            }
            "--tcp" => {
                // TCP mode for testing outside of VMs
                use_tcp = true;
                i += 1;
            }
            "--help" | "-h" => {
                println!("chelsea-agent - In-VM management agent");
                println!();
                println!("Usage: chelsea-agent [OPTIONS]");
                println!();
                println!("Options:");
                println!(
                    "  -p, --port <PORT>  Port to listen on (default: {})",
                    DEFAULT_PORT
                );
                println!("      --tcp          Use TCP instead of vsock (for testing)");
                println!("  -h, --help         Print help");
                std::process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    (port, use_tcp)
}

/// Handle a single client connection.
async fn handle_connection<S>(stream: S, handler: Arc<AgentHandler>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (reader, writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);
    let mut line = String::new();

    if let Err(e) = send_ready_event(&handler, &mut writer).await {
        warn!("Failed to send initial Ready event: {}", e);
    }

    loop {
        line.clear();

        match reader.read_line(&mut line).await {
            Ok(0) => {
                debug!("Client disconnected");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                debug!("Received request: {}", trimmed);

                let response = match serde_json::from_str::<AgentRequest>(trimmed) {
                    Ok(request) => match handler.dispatch(request, &mut writer).await {
                        Ok(DispatchResult::Streaming) => continue,
                        Ok(DispatchResult::Response(response)) => response,
                        Err(e) => {
                            error!("Handler error: {}", e);
                            break;
                        }
                    },
                    Err(e) => {
                        warn!("Failed to parse request: {}", e);
                        AgentResponse::error(
                            "PARSE_ERROR",
                            format!("Failed to parse request: {}", e),
                        )
                    }
                };

                let response_json = match serde_json::to_string(&response) {
                    Ok(json) => json,
                    Err(e) => {
                        error!("Failed to serialize response: {}", e);
                        continue;
                    }
                };

                debug!("Sending response: {}", response_json);

                if let Err(e) = writer.write_all(response_json.as_bytes()).await {
                    error!("Failed to write response: {}", e);
                    break;
                }
                if let Err(e) = writer.write_all(b"\n").await {
                    error!("Failed to write newline: {}", e);
                    break;
                }
                if let Err(e) = writer.flush().await {
                    error!("Failed to flush response: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("Failed to read from client: {}", e);
                break;
            }
        }
    }
}

async fn send_ready_event<W>(handler: &AgentHandler, writer: &mut W) -> Result<(), io::Error>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let response = handler.ready_response().await;
    let response_json =
        serde_json::to_string(&response).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    writer.write_all(response_json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

/// Run the agent with vsock listener (Linux only).
///
/// Security: only connections from CID 2 (`VMADDR_CID_HOST`) are accepted.
/// This prevents processes inside the guest VM from connecting to the agent
/// and issuing privileged commands (e.g. via vsock loopback). A connection
/// semaphore caps concurrency to defend against resource exhaustion.
#[cfg(target_os = "linux")]
async fn run_vsock(
    port: u32,
    handler: Arc<AgentHandler>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = VsockAddr::new(VMADDR_CID_ANY, port);
    let mut listener = VsockListener::bind(addr)?;
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));

    info!("Chelsea agent listening on vsock port {}", port);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        let peer_cid = peer_addr.cid();

                        // Only accept connections from the host (CID 2).
                        // This blocks privilege-escalation from inside the VM.
                        if peer_cid != VMADDR_CID_HOST {
                            warn!(
                                peer_cid,
                                "Rejected vsock connection from non-host CID"
                            );
                            drop(stream);
                            continue;
                        }

                        let permit = match Arc::clone(&semaphore).try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => {
                                warn!("Connection limit reached, rejecting vsock connection");
                                drop(stream);
                                continue;
                            }
                        };

                        info!("Accepted vsock connection from CID {}", peer_cid);
                        let handler = Arc::clone(&handler);
                        tokio::spawn(async move {
                            handle_connection(stream, handler).await;
                            drop(permit); // release on disconnect
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept vsock connection: {}", e);
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("Received shutdown signal, exiting...");
                break;
            }
        }
    }

    Ok(())
}

/// Stub for vsock on non-Linux platforms.
#[cfg(not(target_os = "linux"))]
async fn run_vsock(
    _port: u32,
    _handler: Arc<AgentHandler>,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("vsock is only supported on Linux. Use --tcp for testing on other platforms.".into())
}

/// Run the agent with TCP listener (for testing).
async fn run_tcp(port: u32, handler: Arc<AgentHandler>) -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));

    info!("Chelsea agent listening on TCP port {} (test mode)", port);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        let permit = match Arc::clone(&semaphore).try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => {
                                warn!("Connection limit reached, rejecting TCP connection from {}", peer_addr);
                                drop(stream);
                                continue;
                            }
                        };

                        info!("Accepted TCP connection from {}", peer_addr);
                        let handler = Arc::clone(&handler);
                        tokio::spawn(async move {
                            handle_connection(stream, handler).await;
                            drop(permit);
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept TCP connection: {}", e);
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("Received shutdown signal, exiting...");
                break;
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let (port, use_tcp) = parse_args();

    info!("Starting chelsea-agent v{}", env!("CARGO_PKG_VERSION"));

    let handler = Arc::new(AgentHandler::new());
    info!("Capabilities: {:?}", handler.capabilities());

    // On non-Linux platforms, default to TCP mode
    #[cfg(not(target_os = "linux"))]
    let use_tcp = if !use_tcp {
        warn!("vsock not available on this platform, falling back to TCP mode");
        true
    } else {
        use_tcp
    };

    if use_tcp {
        run_tcp(port, handler).await
    } else {
        run_vsock(port, handler).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::AgentResponse;
    use agent_protocol::{Capability, ReadyResponse};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    type ClientReader = BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>;
    type ClientWriter = tokio::io::WriteHalf<tokio::io::DuplexStream>;

    /// Helper: spin up `handle_connection` on a duplex stream, return the
    /// client half plus the initial Ready response.
    async fn connect() -> (ClientReader, ClientWriter, ReadyResponse) {
        // 64 KB buffer is plenty for these tests
        let (client_stream, agent_stream) = tokio::io::duplex(65_536);
        let handler = Arc::new(AgentHandler::new());

        tokio::spawn(async move {
            handle_connection(agent_stream, handler).await;
        });

        let (read_half, write_half) = tokio::io::split(client_stream);
        let mut reader = BufReader::new(read_half);

        // Consume the initial Ready event the agent sends on connect.
        let mut ready_line = String::new();
        reader.read_line(&mut ready_line).await.unwrap();
        let ready = match serde_json::from_str::<AgentResponse>(ready_line.trim()).unwrap() {
            AgentResponse::Ready(r) => r,
            other => panic!("First message should be Ready, got: {other:?}"),
        };

        (reader, write_half, ready)
    }

    /// Send a line, read back one JSON response line, parse it.
    async fn send_and_recv(
        reader: &mut ClientReader,
        writer: &mut ClientWriter,
        message: &str,
    ) -> AgentResponse {
        writer.write_all(message.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        writer.flush().await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    // ── Parse-error resilience tests ────────────────────────────────

    #[tokio::test]
    async fn total_garbage_returns_parse_error_and_connection_survives() {
        let (mut reader, mut writer, _) = connect().await;

        // Send complete nonsense.
        let resp = send_and_recv(&mut reader, &mut writer, "not even json").await;
        assert!(matches!(&resp, AgentResponse::Error(e) if e.code == "PARSE_ERROR"));

        // Connection is still alive — a valid Ping should work.
        let resp = send_and_recv(&mut reader, &mut writer, r#"{"type":"Ping"}"#).await;
        assert!(matches!(resp, AgentResponse::Pong));
    }

    #[tokio::test]
    async fn valid_json_but_wrong_shape_returns_parse_error() {
        let (mut reader, mut writer, _) = connect().await;

        // Valid JSON, but doesn't match the AgentRequest schema at all.
        let resp = send_and_recv(&mut reader, &mut writer, r#"{"foo": 42}"#).await;
        assert!(matches!(&resp, AgentResponse::Error(e) if e.code == "PARSE_ERROR"));

        let resp = send_and_recv(&mut reader, &mut writer, r#"{"type":"Ping"}"#).await;
        assert!(matches!(resp, AgentResponse::Pong));
    }

    #[tokio::test]
    async fn unknown_type_with_payload_returns_parse_error() {
        let (mut reader, mut writer, _) = connect().await;

        // serde's #[serde(other)] only catches unit variants. An unknown type
        // with a payload is a parse error — verify the agent handles it.
        let resp = send_and_recv(
            &mut reader,
            &mut writer,
            r#"{"type":"FutureFeature","payload":{"data":123}}"#,
        )
        .await;
        assert!(matches!(&resp, AgentResponse::Error(e) if e.code == "PARSE_ERROR"));

        let resp = send_and_recv(&mut reader, &mut writer, r#"{"type":"Ping"}"#).await;
        assert!(matches!(resp, AgentResponse::Pong));
    }

    #[tokio::test]
    async fn unknown_type_without_payload_returns_unsupported() {
        let (mut reader, mut writer, _) = connect().await;

        // Unknown type without payload deserializes to AgentRequest::Unknown,
        // which dispatch handles as UNSUPPORTED (not a parse error).
        let resp = send_and_recv(&mut reader, &mut writer, r#"{"type":"FutureFeature"}"#).await;
        assert!(matches!(&resp, AgentResponse::Error(e) if e.code == "UNSUPPORTED"));

        let resp = send_and_recv(&mut reader, &mut writer, r#"{"type":"Ping"}"#).await;
        assert!(matches!(resp, AgentResponse::Pong));
    }

    #[tokio::test]
    async fn multiple_bad_messages_do_not_accumulate_damage() {
        let (mut reader, mut writer, _) = connect().await;

        // Rapid-fire a mix of garbage, then confirm the agent is still healthy.
        for garbage in [
            "}}}}",
            "",
            "null",
            r#"{"type":""}"#,
            r#"{"type":"Exec"}"#,                // missing required payload
            r#"{"type":"Exec","payload":null}"#, // null payload
            "\x00\x01\x02",                      // binary garbage
        ] {
            writer.write_all(garbage.as_bytes()).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
        }
        writer.flush().await.unwrap();

        // Drain all error responses (empty lines are skipped by the agent).
        let mut error_count = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            if line.trim().is_empty() {
                continue;
            }
            let resp: AgentResponse = serde_json::from_str(line.trim()).unwrap();
            assert!(resp.is_error(), "Expected error, got: {line}");
            error_count += 1;
            if error_count >= 6 {
                // 7 inputs minus the empty string (skipped) = 6 error responses
                break;
            }
        }

        // Agent is still alive.
        let resp = send_and_recv(&mut reader, &mut writer, r#"{"type":"Ping"}"#).await;
        assert!(matches!(resp, AgentResponse::Pong));
    }

    // ── ConfigureNetwork: not advertised, not reachable ─────────────
    //
    // The handler function exists (handle_configure_network) but the
    // capability is deliberately not registered. These tests verify:
    // 1. The Ready handshake does not advertise ConfigureNetwork.
    // 2. Sending a ConfigureNetwork request is rejected (UNSUPPORTED).
    // 3. The connection remains healthy after the rejection.
    //
    // This matters because the host-side VsockClient already has a
    // configure_network() method wired up — if the capability gate
    // were accidentally removed, the agent would start accepting
    // requests for unimplemented functionality.

    #[tokio::test]
    async fn configure_network_not_advertised_in_ready() {
        let (_reader, _writer, ready) = connect().await;

        assert!(
            !ready.capabilities.contains(&Capability::ConfigureNetwork),
            "ConfigureNetwork must not be advertised; got: {:?}",
            ready.capabilities
        );
    }

    #[tokio::test]
    async fn configure_network_rejected_end_to_end() {
        let (mut reader, mut writer, _) = connect().await;

        let request = serde_json::to_string(&AgentRequest::ConfigureNetwork(
            agent_protocol::ConfigureNetworkRequest {
                ip_address: "10.0.0.2/24".to_string(),
                gateway: "10.0.0.1".to_string(),
                dns_servers: vec!["8.8.8.8".to_string()],
            },
        ))
        .unwrap();

        let resp = send_and_recv(&mut reader, &mut writer, &request).await;
        assert!(
            matches!(&resp, AgentResponse::Error(e) if e.code == "UNSUPPORTED"),
            "Expected UNSUPPORTED, got: {resp:?}"
        );

        // Connection is still alive after the rejection.
        let resp = send_and_recv(&mut reader, &mut writer, r#"{"type":"Ping"}"#).await;
        assert!(matches!(resp, AgentResponse::Pong));
    }

    // ── SSH key install: user field is ignored ──────────────────────
    //
    // The InstallSshKeyRequest has a `user` field (defaults to "root"),
    // but the handler always writes to the hardcoded path
    // /root/.ssh/authorized_keys regardless. The `user` field is dead
    // code today. These tests verify that different user values produce
    // identical behavior — the same error (in test env we can't write
    // to /root/.ssh) or, if we could, the same path.
    //
    // This addresses the review feedback that user *names* are not
    // authoritative identifiers (any account can have the name "root").
    // Since we don't resolve home dirs from the user field at all, the
    // concern does not apply to the current implementation.

    #[tokio::test]
    async fn ssh_key_install_ignores_user_field() {
        let (mut reader, mut writer, _) = connect().await;

        // Request with user="root" (the default).
        let req_root = serde_json::to_string(&AgentRequest::InstallSshKey(
            agent_protocol::InstallSshKeyRequest {
                public_key: "ssh-ed25519 AAAA... test@host".to_string(),
                user: "root".to_string(),
            },
        ))
        .unwrap();

        // Request with a completely different user — should hit the exact
        // same code path because the handler ignores the field.
        let req_other = serde_json::to_string(&AgentRequest::InstallSshKey(
            agent_protocol::InstallSshKeyRequest {
                public_key: "ssh-ed25519 AAAA... test@host".to_string(),
                user: "mallory".to_string(),
            },
        ))
        .unwrap();

        let resp_root = send_and_recv(&mut reader, &mut writer, &req_root).await;
        let resp_other = send_and_recv(&mut reader, &mut writer, &req_other).await;

        // In a test environment (not running as root), both attempts will
        // either succeed identically (if /root/.ssh is writable) or fail
        // identically (same error code, same path in the message). The
        // point is they are indistinguishable — the user field changes nothing.
        match (&resp_root, &resp_other) {
            (AgentResponse::Ok, AgentResponse::Ok) => {
                // Both wrote to the same hardcoded path — user was ignored.
            }
            (AgentResponse::Error(e1), AgentResponse::Error(e2)) => {
                assert_eq!(
                    e1.code, e2.code,
                    "Different user values should produce the same error code"
                );
            }
            _ => panic!(
                "Responses should match.\n  user=root:    {resp_root:?}\n  user=mallory: {resp_other:?}"
            ),
        }
    }

    #[tokio::test]
    async fn ssh_key_install_user_field_not_in_error_path() {
        let (mut reader, mut writer, _) = connect().await;

        // Use an absurd user name to make sure it doesn't appear in
        // any error message — proving the handler never tries to
        // resolve it to a home directory or UID.
        let req = serde_json::to_string(&AgentRequest::InstallSshKey(
            agent_protocol::InstallSshKeyRequest {
                public_key: "ssh-ed25519 AAAA... test@host".to_string(),
                user: "CANARY_USER_xyzzy_42".to_string(),
            },
        ))
        .unwrap();

        let resp = send_and_recv(&mut reader, &mut writer, &req).await;

        if let AgentResponse::Error(e) = &resp {
            assert!(
                !e.message.contains("CANARY_USER_xyzzy_42"),
                "The user field should not appear in error messages, \
                 meaning the handler never tried to resolve it. Got: {}",
                e.message
            );
        }
        // If Ok, the handler wrote to /root/.ssh without consulting the user field — also fine.
    }
}
