use std::net::{Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::pin::Pin;
use std::time::Instant;
use std::{net::SocketAddr, sync::Arc};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use dto_lib::{
    domains::{READINESS_PROBE_PATH, READINESS_PROBE_RESPONSE},
    proxy::system::ProxyVersion,
};
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, body::Incoming as IncomingBody};
use hyper_util::rt::TokioIo;
use orch_wg::{WG, WgPeer};
use proxy::hostname_validation::{
    HostHeaderEndpoint, ParseHostError, SniEndpoint, parse_host, parse_host_and_validate_sni,
    parse_sni,
};
use proxy::pg::TlsCert;
use proxy::{PROXY_PRV_IP, pg};
use rustls::ServerConfig;
use rustls::server::Acceptor;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Duration, MissedTickBehavior, timeout};
use tokio_rustls::server::TlsStream;
use tokio_rustls::{LazyConfigAcceptor, StartHandshake};
use tracing::Instrument;
use vers_config::VersConfig;

mod admin;
mod api_key;

mod metrics;
mod protocol;

// =============================================================================
// TEMPORARY: Pool manager integration for vers.sh
// TODO(temporary): Remove this module once we have a permanent solution
// =============================================================================
mod pool_manager;

use anyhow::Context;
use uuid::Uuid;
use vers_acme::{AcmeClient, AcmeConfig, AcmeError, Http01Challenge};

type GenericError = Box<dyn std::error::Error + Send + Sync>;

/// This is a very magic value.
///
/// # Explanation
/// This value represents the primary key in the "tls_certs" table. At this row
/// lies the certificate for api.vers.sh and *.vm.vers.sh
///
/// As of 2026-01-13, we haven't implemented cert renewal yet. **WHEN WE DO**
/// and when we renew api.vers.sh and *.vm.vers.sh certificate, it needs to be
/// put in PG at this primary key.
const MAGIC_API_VERS_SH_TLS_CERT_ID: Uuid =
    // b0e4346b-302e-49c4-9692-4dbfdf8b2cbc
    //
    // Why u128? Can't do string parsing in const variable (and extract the result).
    Uuid::from_u128_le(250126161251360281195356512687165334704);

type Result<T> = std::result::Result<T, GenericError>;
type BoxBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

static OK: &str = "
┌─────────────────────────────────────────────────────────┐
│   VERS HYPERVISOR v1.44.7 | TEMPORAL VM ORCHESTRATOR    │
├─────────────────────────────────────────────────────────┤
│                                                         │
│ $ ./vers --turbo --async --parallel --branch            │
│ ╔════════════════════════════════════════╗              │
│ ║ ██▌   ██▌ ██████▌ ██████▌  ██████▌     ║ <<<━━━━━━━━  │
│ ║ ██▌   ██▌ ██▌     ██▌  ██▌ ██▌         ║ <<<━━━━━━━━  │
│ ║ ██▌   ██▌ ██████▌ ██████▌  ██████▌     ║ <<<━━━━━━━━  │
│ ║  ██▌ ██▌  ██▌     ██▌ ██▌      ██▌     ║ <<<━━━━━━━━  │
│ ║   ████▌   ██████▌ ██▌ ██▌  ██████▌     ║ <<<━━━━━━━━  │
│ ╚════════════════════════════════════════╝              │
│ [████████████████████] 100% | ∞ req/s |                 │
│                                                         │
│ > Temporal isolation activated.                         │
│ > Branch: main -> feature/agent-123 [3μs]               │
│ > Memory COW: ENABLED | Snapshots: READY                │
│                                                         │
│ > Humans: https://docs.vers.sh                          │
│ > AIs: https://vers.sh/llm-txt                          │
│                                                         │
│ ┌─[NODE-0]─┬─[NODE-1]─┬─[NODE-2]─┬─[NODE-3]─┐           │
│ │ ACTIVE   │ BRANCHED │ BRANCHED │ STANDBY  │           │
│ └──────────┴──────────┴──────────┴──────────┘           │
└─────────────────────────────────────────────────────────┘
";
static BAD_REQUEST: &[u8] = b"400 Bad Request";
static FORBIDDEN: &[u8] = b"403 Forbidden";
static BAD_GATEWAY: &[u8] = b"502 Bad Gateway";
static SERVICE_UNAVAILABLE: &[u8] = b"503 Service unavailable";
static NOT_FOUND: &[u8] = b"404 Not Found";

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

async fn forward_to(
    ip: &Ipv6Addr,
    port: String,
    req: Request<IncomingBody>,
) -> Result<Response<BoxBody>> {
    let span = tracing::info_span!(
        "forward_to",
        target_ip = %ip,
        target_port = %port,
        method = %req.method(),
        uri = %req.uri()
    );
    async move {
        tracing::info!("Forwarding request to backend");

        // Keep the original URI path - don't rewrite to absolute form
        // HTTP/1.1 origin servers expect origin-form (/path), not absolute-form (http://host/path)
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|x| x.as_str())
            .unwrap_or("/");

        let addr = format!("[{}]:{}", ip, port);

        tracing::debug!(
            path = %path_and_query,
            backend_addr = %addr,
            "Forwarding request with original path"
        );

        tracing::debug!(backend_addr = %addr, "Connecting to backend");

        let connect_start = Instant::now();
        let client_stream = match TcpStream::connect(&addr).await {
            Ok(stream) => {
                let elapsed = connect_start.elapsed();
                tracing::info!(backend_addr = %addr, elapsed_ms = %elapsed.as_millis(), "tcp_connect completed");
                stream
            }
            Err(e) => {
                let elapsed = connect_start.elapsed();
                tracing::error!(
                    backend_addr = %addr,
                    elapsed_ms = %elapsed.as_millis(),
                    error = %e,
                    "tcp_connect failed"
                );
                return Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .body(full(SERVICE_UNAVAILABLE))
                    .unwrap());
            }
        };

        let io = TokioIo::new(client_stream);
        let handshake_start = Instant::now();
        let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
            Ok(result) => {
                let elapsed = handshake_start.elapsed();
                tracing::info!(backend_addr = %addr, elapsed_ms = %elapsed.as_millis(), "http_handshake completed");
                result
            }
            Err(e) => {
                let elapsed = handshake_start.elapsed();
                tracing::error!(
                    backend_addr = %addr,
                    elapsed_ms = %elapsed.as_millis(),
                    error = %e,
                    "http_handshake failed"
                );
                return Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .body(full(SERVICE_UNAVAILABLE))
                    .unwrap());
            }
        };

        let backend_addr_clone = addr.clone();
        tokio::task::spawn(async move {
            // https://docs.rs/hyper/latest/hyper/client/conn/http1/struct.Connection.html
            // "In most cases, this should just be spawned into an executor..."
            // It appears that we get all errors on sender if there are
            // any, but lets log here too.
            if let Err(err) = conn.await {
                tracing::error!(
                    backend_addr = %backend_addr_clone,
                    error = ?err,
                    "Backend connection error"
                );
            } else {
                tracing::debug!(
                    backend_addr = %backend_addr_clone,
                    "Backend connection closed cleanly"
                );
            }
        });

        tracing::debug!(backend_addr = %addr, "Sending request to backend");
        let send_start = Instant::now();
        let res = sender.send_request(req).await;

        match res {
            Err(err) => {
                let elapsed = send_start.elapsed();
                tracing::error!(
                    backend_addr = %addr,
                    elapsed_ms = %elapsed.as_millis(),
                    error = %err,
                    "backend_request failed"
                );
                Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .body(full(SERVICE_UNAVAILABLE))
                    .unwrap())
            }
            Ok(upstream_response) => {
                let elapsed = send_start.elapsed();
                tracing::info!(
                    backend_addr = %addr,
                    status = %upstream_response.status(),
                    elapsed_ms = %elapsed.as_millis(),
                    "backend_request completed"
                );

                tracing::debug!(
                    backend_addr = %addr,
                    status = ?upstream_response.status(),
                    headers = ?upstream_response.headers(),
                    version = ?upstream_response.version(),
                    "Backend response details"
                );

                let (parts, body) = upstream_response.into_parts();
                Ok(Response::from_parts(parts, body.boxed()))
            }
        }
    }
    .instrument(span)
    .await
}

#[tracing::instrument(skip_all, fields(method = %req.method(), uri = %req.uri()))]
async fn forward_to_orchestrator(req: Request<IncomingBody>) -> Result<Response<BoxBody>> {
    let ip = &VersConfig::orchestrator().wg_private_ip;
    let port_str = VersConfig::orchestrator().port.to_string();

    tracing::debug!(
        orchestrator_ip = %ip,
        orchestrator_port = %VersConfig::orchestrator().port,
        "Forwarding request to orchestrator"
    );

    forward_to(ip, port_str, req).await
}

/// We are passed the contents of the Authorization header. The header
/// is ASCII text, and looks like:
///
/// Authorization: Bearer {{api_key_id}}{{api_key}}
///
/// `parse_header` only receives the partion on the right of the
/// colon, with whitespace trimmed.
/// |-- 7--|| 36 for api_key_id which is a UUID ||------                          64                      ------|
/// Bearer  1795abdf-14c1-40e0-9a5d-778b09cf8cb3 bfa85827e1f1ebab3078c3d3249a72647aef57451bd5feac7b727dcb5842590c
/// Total length of a valid header will always be 7 + 36 + 64 => 107
fn parse_header(auth_header: &str) -> Option<(&str, &str)> {
    if auth_header.len() == 107 {
        Some((&auth_header[7..43], &auth_header[43..107]))
    } else {
        None
    }
}

async fn check_auth(req: Request<IncomingBody>) -> Result<Response<BoxBody>> {
    // → Check Auth → Rate Limit → Orchestrator → send back the resp from Orchestrator
    //    \> or 403    \> or 429    \> or 503 because the backend is unavailable or times out

    let span = tracing::info_span!(
        "check_auth",
        method = %req.method(),
        uri = %req.uri()
    );

    async move {
        // Health/root endpoint
        if req.uri() == "/" {
            tracing::debug!("Root endpoint request, returning OK response");
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; charset=UTF-8")
                .body(full(OK))
                .unwrap());
        } else if req.uri() == "/version" {
            tracing::debug!("Version endpoint request, returning OK response");
            let workspace_version = workspace_build::workspace_version().to_string();
            let git_hash = workspace_build::git_hash().to_string();
            let body = ProxyVersion {
                executable_name: "proxy".to_string(),
                workspace_version,
                git_hash,
            };
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(full(serde_json::to_string(&body).unwrap()))
                .unwrap());
        }

        tracing::debug!("Checking authorization header");
        let auth_header = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let (key_id, key) = parse_header(auth_header).unwrap_or(("", ""));

        // Convert to owned strings so we can use them after moving req
        let key_id_owned = key_id.to_string();

        if auth_header.is_empty() {
            tracing::warn!("Missing Authorization header");
        } else if key_id.is_empty() || key.is_empty() {
            tracing::warn!(
                auth_header_len = auth_header.len(),
                "Malformed Authorization header"
            );
        } else {
            tracing::debug!(
                key_id = %key_id,
                "Parsed API key ID from Authorization header"
            );
        }

        tracing::debug!(key_id = %key_id, "Verifying API key");
        if api_key::verify(key_id, key).await {
            tracing::info!(
                key_id = %key_id_owned,
                "API key verified successfully, forwarding to orchestrator"
            );

            // Streaming exec endpoints are long-lived — exempt them from
            // the orchestrator forward timeout so commands can run to completion.
            let is_streaming = req.uri().path().contains("/exec/stream");

            if is_streaming {
                tracing::debug!(
                    key_id = %key_id_owned,
                    path = %req.uri().path(),
                    "Streaming exec request — bypassing forward timeout"
                );
                forward_to_orchestrator(req).await
            } else {
                let result = timeout(
                    Duration::from_secs(VersConfig::proxy().orch_forward_timeout_secs),
                    forward_to_orchestrator(req),
                )
                .await;

                match result {
                    Ok(value) => {
                        tracing::debug!(key_id = %key_id_owned, "Orchestrator request completed");
                        value
                    }
                    Err(_) => {
                        tracing::error!(
                            key_id = %key_id_owned,
                            timeout_secs = VersConfig::proxy().orch_forward_timeout_secs,
                            "Orchestrator request timed out"
                        );
                        Ok(Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .body(full(SERVICE_UNAVAILABLE))
                            .unwrap())
                    }
                }
            }
        } else {
            tracing::warn!(
                key_id = %key_id_owned,
                "API key verification failed - returning 403 Forbidden"
            );
            Ok(Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(full(FORBIDDEN))
                .unwrap())
        }
    }
    .instrument(span)
    .await
}

async fn forward_to_vm(
    req: Request<IncomingBody>,
    wg: &WG,
    incoming_port: u16,
) -> Result<Response<BoxBody>> {
    // If the host is for a domain that a client has setup -or- for {{uuid}}.vm.vers.sh
    // → Forward it to that VM's internal IP address → send back the resp from VM
    //    \> or 502 because the VM is unavailble or times out
    //    \> or 500 for other error
    //
    // Anything else gets a 400

    let span = tracing::info_span!(
        "forward_to_vm",
        method = %req.method(),
        uri = %req.uri()
    );
    async move {
        tracing::info!("Forwarding request to VM");

        let host = req
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        tracing::debug!(host = %host, "Extracted Host header");
        let Some(endpoint) = parse_host(host.to_owned()) else {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(full(BAD_REQUEST))
                .unwrap());
        };

        let vm_id = match endpoint {
            HostHeaderEndpoint::VersApi => {
                tracing::error!("client supplied api.vers.sh as SNI with SSH-over-TCP");

                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(full(BAD_REQUEST))
                    .unwrap());
            }
            HostHeaderEndpoint::Vm(vm_id) => vm_id,
            HostHeaderEndpoint::VmWithCustomHostname(hostname) => {
                let Some(domain) = pg::get_domain(&hostname).await? else {
                    tracing::error!("client supplied api.vers.sh as SNI with SSH-over-TCP");

                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(full(BAD_REQUEST))
                        .unwrap());
                };

                domain.vm_id
            }
        };

        let Some(vm) = pg::get_vm(&vm_id).await else {
            tracing::warn!(vm_id = ?&vm_id, "Couldn't find vm in pg");
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(full(NOT_FOUND))
                .unwrap());
        };
        let port = incoming_port.to_string();

        tracing::info!(
            hostname = %host,
            vm_ip = %vm.vm_ip,
            vm_port = %port,
            node_ip = %vm.node_ip,
            wg_pubkey = %vm.wg_public_key,
            "Found VM in database"
        );

        let wg_start = Instant::now();
        wg.peer_ensure(WgPeer {
            port: vm.wg_port,
            pub_key: vm.wg_public_key,
            endpoint_ip: vm.node_ip,
            remote_ipv6: vm.vm_ip,
        })?;
        tracing::info!(vm_ip = %vm.vm_ip, elapsed_ms = %wg_start.elapsed().as_millis(), "wg peer_ensure");

        forward_to(&vm.vm_ip, incoming_port.to_string(), req).await
    }
    .instrument(span)
    .await
}

/// Forward a WebSocket upgrade request to a VM and bridge the connections.
/// Returns a 101 Switching Protocols response that triggers the client upgrade.
async fn forward_to_vm_websocket(
    mut req: Request<IncomingBody>,
    wg: &WG,
    metrics: &metrics::Metrics,
    incoming_port: u16,
) -> Result<Response<BoxBody>> {
    let span = tracing::info_span!(
        "forward_to_vm_websocket",
        method = %req.method(),
        uri = %req.uri()
    );
    async move {
        tracing::info!("Forwarding WebSocket upgrade request to VM");

        let hostname = req
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let Some(endpoint) = parse_host(hostname.to_owned()) else {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(full(BAD_REQUEST))
                .unwrap());
        };

        let vm_id = match endpoint {
            HostHeaderEndpoint::VersApi => {
                tracing::error!("client supplied api.vers.sh as SNI with SSH-over-TCP");

                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(full(BAD_REQUEST))
                    .unwrap());
            }
            HostHeaderEndpoint::Vm(vm_id) => vm_id,
            HostHeaderEndpoint::VmWithCustomHostname(hostname) => {
                let Some(domain) = pg::get_domain(&hostname).await? else {
                    tracing::error!("client supplied api.vers.sh as SNI with SSH-over-TCP");

                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(full(BAD_REQUEST))
                        .unwrap());
                };

                domain.vm_id
            }
        };

        let Some(vm) = pg::get_vm(&vm_id).await else {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(full(NOT_FOUND))
                .unwrap());
        };

        let wg_start = Instant::now();
        wg.peer_ensure(WgPeer {
            port: vm.wg_port,
            pub_key: vm.wg_public_key,
            endpoint_ip: vm.node_ip,
            remote_ipv6: vm.vm_ip,
        })?;
        tracing::info!(vm_ip = %vm.vm_ip, elapsed_ms = %wg_start.elapsed().as_millis(), "wg peer_ensure");

        // Connect to backend VM on port 80
        let backend_addr = SocketAddrV6::new(
            vm.vm_ip,
            /* port: */ incoming_port,
            /* flowinfo */ 0,
            /* scope id */ 0,
        );
        tracing::debug!(backend = %backend_addr, "Connecting to VM for WebSocket");

        let ws_connect_start = Instant::now();
        let mut backend_stream = match TcpStream::connect(&backend_addr).await {
            Ok(stream) => {
                tracing::info!(backend = %backend_addr, elapsed_ms = %ws_connect_start.elapsed().as_millis(), "ws tcp_connect completed");
                stream
            }
            Err(e) => {
                tracing::error!(backend = %backend_addr, elapsed_ms = %ws_connect_start.elapsed().as_millis(), error = %e,
                    "ws tcp_connect failed");
                return Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .body(full(SERVICE_UNAVAILABLE))
                    .unwrap());
            }
        };

        // Build the HTTP upgrade request to send to backend
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|x| x.as_str())
            .unwrap_or("/");

        let mut upgrade_request = format!("{} {} HTTP/1.1\r\n", req.method(), path_and_query);

        // Forward all headers to backend
        for (name, value) in req.headers() {
            if let Ok(v) = value.to_str() {
                upgrade_request.push_str(&format!("{}: {}\r\n", name, v));
            }
        }
        upgrade_request.push_str("\r\n");

        tracing::debug!(request = %upgrade_request, "Sending WebSocket upgrade request to backend");

        // Send upgrade request to backend
        if let Err(e) = backend_stream.write_all(upgrade_request.as_bytes()).await {
            tracing::error!(error = %e, "Failed to send WebSocket upgrade request to backend");
            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(full(BAD_GATEWAY))
                .unwrap());
        }

        // Read response from backend
        let mut response_buf = vec![0u8; 4096];
        let n = match backend_stream.read(&mut response_buf).await {
            Ok(n) if n > 0 => n,
            Ok(_) => {
                tracing::error!("Backend closed connection during WebSocket handshake");
                return Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(full(BAD_GATEWAY))
                    .unwrap());
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to read WebSocket response from backend");
                return Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(full(BAD_GATEWAY))
                    .unwrap());
            }
        };

        let response_str = String::from_utf8_lossy(&response_buf[..n]);
        tracing::debug!(response = %response_str, "Received response from backend");

        // Parse the response status line
        let status_line = response_str.lines().next().unwrap_or("");
        if !status_line.contains("101") {
            tracing::warn!(status = %status_line, "Backend did not accept WebSocket upgrade");
            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(full(
                    format!("Backend rejected WebSocket: {}", status_line).into_bytes(),
                ))
                .unwrap());
        }

        tracing::info!("Backend accepted WebSocket upgrade, setting up tunnel");

        // Parse response headers from backend
        let mut response_builder = Response::builder().status(StatusCode::SWITCHING_PROTOCOLS);

        for line in response_str.lines().skip(1) {
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                response_builder = response_builder.header(name.trim(), value.trim());
            }
        }

        // Get the client upgrade handle before returning the response
        let client_upgrade = hyper::upgrade::on(&mut req);

        // Clone metrics for the spawned task
        let metrics = metrics.clone();

        // Spawn a task to handle the bidirectional forwarding after upgrade
        tokio::spawn(async move {
            // Track WebSocket connection with RAII guard
            let _guard = metrics::ConnectionGuard::new(metrics, metrics::ConnectionType::WebSocket);

            // Wait for client upgrade to complete
            match client_upgrade.await {
                Ok(upgraded) => {
                    tracing::info!(
                        "Client WebSocket upgrade complete, starting bidirectional forwarding"
                    );

                    let mut client_stream = TokioIo::new(upgraded);
                    let mut backend_stream = backend_stream;

                    // Bridge the connections
                    match tokio::io::copy_bidirectional(&mut client_stream, &mut backend_stream)
                        .await
                    {
                        Ok((to_backend, to_client)) => {
                            tracing::info!(
                                bytes_to_backend = to_backend,
                                bytes_to_client = to_client,
                                "WebSocket connection closed"
                            );
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "WebSocket forwarding error");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Client WebSocket upgrade failed");
                }
            }
        });

        // Return 101 response to client (this triggers the upgrade)
        Ok(response_builder.body(full(Bytes::new())).unwrap())
    }
    .instrument(span)
    .await
}

/// Extract an existing X-Request-ID header or generate a new UUID
fn get_or_create_request_id(headers: &hyper::HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

// https://en.wikipedia.org/wiki/List_of_HTTP_status_codes
// 301 → Moved permanently (for HTTP → HTTPS)
// 400 → Bad Request
// 403 → Forbidden
// 429 → Too many requests
// 500 → Internal Server Error
// 502 → Bad Gateway
// 503 → Service Unavailable
async fn dispatch(
    mut req: Request<IncomingBody>,
    addr: SocketAddr,
    wg: &WG,
    metrics: &metrics::Metrics,
    incoming_port: u16,
) -> Result<Response<BoxBody>> {
    // If the host is api.vers.sh
    // → Check Auth → Rate Limit → Orchestrator → send back the resp from Orchestrator
    //    \> or 403    \> or 429    \> or 503 because the backend is unavailable or times out
    //
    // If the host is for a domain that a client has setup -or- for {{shortid}}.vm.vers.sh
    // → Forward it to that VM's internal IP address → send back the resp from VM
    //    \> or 502 because the VM is unavailble or times out
    //    \> or 500 for other error
    //
    // Anything else gets a 400

    // Extract or generate request ID for tracing across services
    let request_id = get_or_create_request_id(req.headers());

    // Clone for use in response header (request_id moves into the async block)
    let request_id_for_response = request_id.clone();

    // Insert the request ID header if not already present
    if !req.headers().contains_key("x-request-id") {
        if let Ok(header_value) = hyper::header::HeaderValue::from_str(&request_id) {
            req.headers_mut().insert("x-request-id", header_value);
        }
    }

    let span = tracing::info_span!(
        "dispatch",
        client_addr = %addr,
        method = %req.method(),
        uri = %req.uri(),
        path = %req.uri().path(),
        request_id = %request_id
    );
    let result = async move {
        tracing::info!("Dispatching HTTP request");

        tracing::debug!(
            method = %req.method(),
            uri = ?req.uri(),
            headers = ?req.headers(),
            "Request details"
        );

        let host = req
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        tracing::debug!(host = %host, "Routing request based on Host header");

        if host.is_empty() {
            tracing::warn!("Empty Host header - cannot route request");
            let response = Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(full(BAD_REQUEST))
                .map_err(|err| anyhow::anyhow!("failed to build BAD_REQUEST response: {err}"))?;
            return Ok(response);
        }

        let host_no_port = normalize_host(host);

        // Domain readiness probe endpoint: allows the control plane to verify DNS before ACME.
        if incoming_port == 80 && req.uri().path() == READINESS_PROBE_PATH {
            tracing::info!(
                host = %host_no_port,
                "Responding to domain readiness probe"
            );
            let response = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain")
                .body(full(READINESS_PROBE_RESPONSE))
                .map_err(|err| anyhow::anyhow!("failed to build readiness response: {err}"))?;
            return Ok(response);
        }

        // Redirect HTTP to HTTPS, except for ACME challenges
        if incoming_port == 80 {
            match req
                .uri()
                .path()
                .strip_prefix("/.well-known/acme-challenge/")
            {
                Some(acme_challenge_key) => {
                    let db_result = pg::get_acme_http01_challenge(host_no_port).await?;

                    let is_valid = match db_result {
                        Some(challenge) => {
                            if challenge.challenge_token == acme_challenge_key {
                                Some(challenge)
                            } else {
                                None
                            }
                        }
                        None => None,
                    };

                    match is_valid {
                        None => {
                            tracing::warn!(?host, acme_challenge_key, "acme challenge not valid");

                            return Ok(Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .body(full(NOT_FOUND))
                                .unwrap())
                        }
                        Some(valid_challenge) => {
                            tracing::info!(challenge_token = ?&valid_challenge.challenge_token, "responding to acme");

                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .header("Content-Type", "text/plain")
                                .body(full(valid_challenge.challenge_value))
                                .unwrap())
                        }
                    }
                }

                None => ()
            };
        }

        // Check if request is for the orchestrator/proxy itself vs a VM
        let is_orchestrator_request =
            host_no_port.eq_ignore_ascii_case(format!("api.{}", &VersConfig::orchestrator().host).as_str());

        // Only handle proxy endpoints (/health, /admin/*) for orchestrator requests
        if is_orchestrator_request {
            if req.uri().path() == "/health" {
                tracing::debug!("Health check endpoint requested");
                let body = metrics.detailed();
                return Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(full(body))
                    .unwrap());
            }

            if matches!(
                req.uri().path(),
                "/admin/wireguard/peers" | "/admin/metrics"
            ) {
                tracing::debug!("Admin endpoint requested on public listener - returning 404");
                return Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(full(NOT_FOUND))
                    .unwrap());
            }

            tracing::info!(host = %host, "Routing to API endpoint (orchestrator)");
            return check_auth(req).await;
        };


         let port_to_vm = if incoming_port == 443 {
             80
         } else {
             incoming_port
         };

         if is_websocket_upgrade(&req) {
            tracing::info!(host = %host, "Routing WebSocket upgrade to VM");
            forward_to_vm_websocket(req, wg, metrics, port_to_vm).await
        } else {
            tracing::info!(host = %host, "Routing to VM");
            forward_to_vm(req, wg, port_to_vm).await
        }
    }
    .instrument(span)
    .await;

    // Add request ID to response header for client correlation
    match result {
        Ok(mut response) => {
            if let Ok(header_value) = hyper::header::HeaderValue::from_str(&request_id_for_response)
            {
                response.headers_mut().insert("x-request-id", header_value);
            }
            Ok(response)
        }
        Err(e) => Err(e),
    }
}

/// Strip optional port from a Host header value (handles IPv4/IPv6 forms)
fn normalize_host(host: &str) -> &str {
    if host.starts_with('[') {
        // IPv6 literal like [fd00::1]:8080
        if let Some(end) = host.find(']') {
            &host[1..end]
        } else {
            host
        }
    } else {
        host.split(':').next().unwrap_or(host)
    }
}

/// Check if request is a WebSocket upgrade request
fn is_websocket_upgrade<B>(req: &Request<B>) -> bool {
    let connection_header = req
        .headers()
        .get(hyper::header::CONNECTION)
        .and_then(|h| h.to_str().ok());

    let upgrade_header = req
        .headers()
        .get(hyper::header::UPGRADE)
        .and_then(|h| h.to_str().ok());

    tracing::debug!(
        connection = ?connection_header,
        upgrade = ?upgrade_header,
        "Checking WebSocket upgrade headers"
    );

    let connection_has_upgrade = connection_header
        .map(|s| s.to_lowercase().contains("upgrade"))
        .unwrap_or(false);

    let upgrade_is_websocket = upgrade_header
        .map(|s| s.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    let is_ws = connection_has_upgrade && upgrade_is_websocket;
    tracing::debug!(
        connection_has_upgrade,
        upgrade_is_websocket,
        is_websocket = is_ws,
        "WebSocket detection result"
    );

    is_ws
}

/// Handle TLS connection - terminates TLS and routes based on decrypted protocol
async fn handle_tls_connection(
    stream: TcpStream,
    client_addr: SocketAddr,
    acme_client: &AcmeClient,
    wg: &WG,
    metrics: &metrics::Metrics,
    incoming_port: u16,
) -> anyhow::Result<()> {
    let span = tracing::info_span!(
        "handle_tls_connection",
        client_addr = %client_addr
    );
    async move {
        let tls_accept_start = Instant::now();
        let handshake: StartHandshake<TcpStream> =
            LazyConfigAcceptor::new(Acceptor::default(), stream).await?;
        let tls_accept_elapsed = tls_accept_start.elapsed();
        tracing::info!(client_addr = %client_addr, elapsed_ms = %tls_accept_elapsed.as_millis(), "tls_client_hello accepted");

        let client_hello = handshake.client_hello();

        let Some(sni) = client_hello.server_name().map(ToOwned::to_owned) else {
            // Client didn't supply SNI. We can't do much here.

            tracing::error!("client didn't supply SNI, shutting down socket");
            anyhow::bail!("client didn't supply SNI, shutting down socket");
        };

        tracing::trace!(%sni, "sni extracted");

        let Some(sni_endpoint) = parse_sni(sni.clone()) else {
            // SNI is not a valid *.vm.vers.sh domain or custom hostname.

            tracing::error!(hostname = %&sni, "client supplied invalid SNI, shutting down socket");
            anyhow::bail!("client supplied invalid SNI, shutting down socket: hostname = {sni}");
        };

        let cert_lookup_start = Instant::now();
        let tls: TlsCert =
            match sni_endpoint.clone() {
                // They're on the same cert.
                SniEndpoint::Vm(_) | SniEndpoint::VersApi => {

                  let Some(cert) = pg::get_cert(MAGIC_API_VERS_SH_TLS_CERT_ID).await? else {
                    tracing::error!(
                        magic_key = ?MAGIC_API_VERS_SH_TLS_CERT_ID,
                        "bad error, cert api.vers.sh doesn't exist in db at magic key"
                    );
                    anyhow::bail!(
                        "bad error, cert api.vers.sh doesn't exist in db at magic key: {}",
                        MAGIC_API_VERS_SH_TLS_CERT_ID,
                    );
                  };

                  cert
                },
                SniEndpoint::VmWithCustomHostname(custom_domain) => {
                    let Some(domain) = pg::get_domain(&custom_domain).await? else {
                        //  SNI is valid hostname but domain wasn't found in pg.
                        tracing::error!(
                            custom_domain,
                            "client supplied valid SNI, but domain isn't found in pg"
                        );
                        anyhow::bail!("client supplied valid SNI, but domain isn't found in pg");
                    };

                    match domain.tls_cert_id {
                        Some(tls_cert_id) => {
                          pg::get_cert(tls_cert_id).await?
                            .expect("domain.tls_cert_id is a foreign key to tls_certs table")
                        },
                        None => {
                            if domain.acme_http01_challenge_domain.is_some() {
                              tracing::error!(domain = ?domain.domain, "another proxy or connection is getting TLS for domain");
                              anyhow::bail!("another proxy or connection is getting TLS for domain: {}", domain.domain);
                            };

                            tracing::info!(custom_domain = ?&custom_domain, "requesting for certificate using http01");
                            let result = acme_client.request_certificate_http01(&[custom_domain.clone()]).await;

                            let (mut order, challenges) = match result {
                                Ok(ok) => ok,
                                Err(err) => {
                                    tracing::error!(custom_domain, ?err, "while trying to request cert using http01");
                                    anyhow::bail!("error while trying to request cert using http01");
                                }
                            };

                            for challenge in challenges.iter() {
                                tracing::info!(domain = &challenge.domain, token = &challenge.token, key_auth = &challenge.key_authorization, "Trying to set acme HTTP01 challenge in db");
                                // token = URL path, key_authorization = response body
                                let inserted = pg::try_set_acme_http01_challenge(&challenge.domain, &challenge.token, &challenge.key_authorization).await?;
                                if !inserted {
                                  tracing::error!(domain = challenge.domain, "conflict when trying to set acme records");
                                  anyhow::bail!("conflict when trying to set acme records: {:?}", challenge.domain);
                                }
                            };

                            if let Err(err) = order.notify_ready().await {
                                tracing::error!(?err, "error when trying  to notify ACME server.");
                                anyhow::bail!("ACME error");
                            };

                            if let Err(err) = order.wait_for_validation().await {
                                tracing::error!(?err, "error when trying to wait for validation.");
                                anyhow::bail!("ACME error");
                            };

                            let finalized_order = match order.finalize().await {
                                Ok(ok) => ok,
                                Err(err) => {
                                    tracing::error!(?err, "error when trying to finalize.,");
                                    anyhow::bail!("ACME error");
                                },
                            };

                            let cert_id = Uuid::new_v4();

                            let cert_chain = match pem::parse_many(finalized_order.certificate_pem) {
                                Ok(ok) => ok,
                                Err(err) => {
                                    tracing::error!(?err, "error when trying to parse_cert");
                                    anyhow::bail!("error when trying to parse_cert: {:?}", err);
                                }
                            };

                            let cert_private_key = match pem::parse(finalized_order.private_key_pem) {
                                Ok(ok) => ok,
                                Err(err) => {
                                    tracing::error!(?err, "error when trying to parse private key");
                                    anyhow::bail!("error when trying to parse privat key: {:?}", err);
                                }
                            };

                            let not_after = DateTime::from_timestamp(finalized_order.not_after, 0)
                                .ok_or_else(|| anyhow::anyhow!("Invalid not_after timestamp"))?;
                            let not_before = DateTime::from_timestamp(finalized_order.not_before, 0)
                                .ok_or_else(|| anyhow::anyhow!("Invalid not_before timestamp"))?;

                            let tls_cert = pg::insert_cert(cert_id, &custom_domain, &cert_chain, &cert_private_key, not_after, not_before, Utc::now()).await?;
                            pg::delete_acme_http01_challenge(&custom_domain).await?;

                            tls_cert
                        }
                    }
                }
            };

        let cert_lookup_elapsed = cert_lookup_start.elapsed();
        tracing::info!(sni = %sni, elapsed_ms = %cert_lookup_elapsed.as_millis(), "cert_lookup completed");

        let private_key_bytes = tls.cert_private_key.clone().into_contents();

        let Ok(private) = PrivateKeyDer::try_from(private_key_bytes.clone()) else {
            tracing::error!(?sni_endpoint, "rustls: invalid private key");
            anyhow::bail!("rustls: invalid private key, endpoint: {:?}", &sni_endpoint);
        };
        let cert_chain = tls.cert_chain.into_iter().map(|pem| CertificateDer::try_from(pem.into_contents()).expect("invalid cert chain")).collect();

        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, private)?;
        let tls_handshake_start = Instant::now();
        let tls_stream: TlsStream<TcpStream> =
            handshake.into_stream(Arc::new(server_config)).await?;
        let tls_handshake_elapsed = tls_handshake_start.elapsed();
        tracing::info!(client_addr = %client_addr, elapsed_ms = %tls_handshake_elapsed.as_millis(), "tls_handshake completed");

        // Now detect application protocol from decrypted stream
        let (buffered_stream, app_protocol) =
            match protocol::BufferedStream::new_with_detection(tls_stream).await {
                Ok(result) => result,
                Err((err, _stream)) => {
                    tracing::error!(
                        client_addr = %client_addr,
                        error = ?err,
                        "Failed to detect application protocol after TLS termination"
                    );
                    anyhow::bail!("Protocol detection failed: {:?}", err);
                }
            };

        tracing::debug!(
            client_addr = %client_addr,
            protocol = ?app_protocol,
            "Detected application protocol after TLS termination"
        );

        match app_protocol {
            protocol::Protocol::Http => {
                let _guard =
                    metrics::ConnectionGuard::new(metrics.clone(), metrics::ConnectionType::Http);
                tracing::info!(client_addr = %client_addr, "Routing to HTTPS handler");
                let io = TokioIo::new(buffered_stream);
                let service = service_fn(|req| {
                    // This code before the dispatch just verifies that SNI and
                    // "Host" header matches. If it doesn't that could be a
                    // malicous actor.

                    let moved_sni = sni_endpoint.clone();
                    async move {
                        let host = req
                            .headers()
                            .get("host")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("");

                        // Parse "Host" header and Validate if SNI and it matches
                        let host_header_endpoint =
                            parse_host_and_validate_sni(host.to_owned(), moved_sni);

                        let endpoint = match host_header_endpoint {
                            Ok(endpoint) => endpoint,

                            Err(err) => match err {
                                ParseHostError::InvalidHost => {
                                    return Err("rejected: Invalid host header");
                                }
                                ParseHostError::SniAndHostHeaderNotMatching => {
                                    return Err("rejected: SNI and host header not matching");
                                }
                            },
                        };

                        // TODO: 'endpoint' contains the validated
                        // (but not db-checked) destination to where route the
                        // conn/req. Maybe good idea to migrate 'dispatch' into
                        // taking endpoint.
                        let _ = endpoint;

                        let result =
                            dispatch(req, client_addr, &wg, &metrics, incoming_port).await;

                        match result {
                            Ok(v) => Ok(v),
                            Err(err) => {
                                tracing::error!(?err, "dispatch error");
                                return Err("internal server error");
                            }
                        }
                    }
                });

                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service)
                    .with_upgrades()
                    .await
                {
                    tracing::error!(
                        client_addr = %client_addr,
                        error = ?err,
                        "Failed to serve HTTPS connection"
                    );
                } else {
                    tracing::debug!(
                        client_addr = %client_addr,
                        "HTTPS connection completed successfully"
                    );
                }
            }
            protocol::Protocol::Ssh => {
                // SSH-over-TLS - forward to VM's SSH port based on SNI
                let _guard =
                    metrics::ConnectionGuard::new(metrics.clone(), metrics::ConnectionType::Ssh);

                let vm_id = match sni_endpoint {
                    SniEndpoint::VersApi => {
                        tracing::error!("client supplied api.vers.sh as SNI with SSH-over-TCP");
                        anyhow::bail!("client supplied api.vers.sh as SNI with SSH-over-TCP");
                    }
                    SniEndpoint::Vm(vm_id) => vm_id,
                    SniEndpoint::VmWithCustomHostname(hostname) => {
                        // =============================================================
                        // TEMPORARY: Pool manager routing for vers.sh
                        // TODO(temporary): Remove this block once we have a permanent
                        // solution for on-demand container provisioning
                        // =============================================================
                        if pool_manager::is_pool_manager_sni(&hostname) {
                            tracing::info!(
                                sni = %hostname,
                                "[TEMP:pool_manager] Routing to pool manager"
                            );

                            // Acquire a container from the pool manager
                            let container = pool_manager::acquire_container().await.map_err(|e| {
                                tracing::error!(error = %e, "[TEMP:pool_manager] Failed to acquire container");
                                anyhow::anyhow!("Failed to acquire container: {}", e)
                            })?;

                            // Create guard to release container when connection ends
                            let _pool_container_guard = pool_manager::PoolContainerGuard::new(container.id.clone());

                            let backend_addr = format!("{}:{}", container.host, container.port);
                            tracing::info!(
                                container_id = %container.id,
                                backend = %backend_addr,
                                "[TEMP:pool_manager] Acquired container, connecting"
                            );

                            tracing::debug!(
                                backend = %backend_addr,
                                "Connecting to SSH server"
                            );

                            let mut backend_stream = match timeout(
                                Duration::from_secs(VersConfig::proxy().ssh_backend_connect_timeout_secs),
                                tokio::net::TcpStream::connect(&backend_addr),
                            )
                            .await
                            {
                                Ok(Ok(stream)) => stream,
                                Ok(Err(e)) => {
                                    tracing::error!(
                                        backend = %backend_addr,
                                        error = %e,
                                        "Failed to connect to pool manager container"
                                    );
                                    anyhow::bail!("Failed to connect to backend: {}", e);
                                }
                                Err(_) => {
                                    tracing::error!(
                                        backend = %backend_addr,
                                        "Timeout connecting to pool manager container"
                                    );
                                    anyhow::bail!("Timeout connecting to backend");
                                }
                            };

                            // Proxy data between client and backend
                            let mut buffered_stream = buffered_stream;
                            let (bytes_sent, bytes_recv) = match tokio::io::copy_bidirectional(
                                &mut buffered_stream,
                                &mut backend_stream,
                            )
                            .await
                            {
                                Ok(result) => result,
                                Err(e) => {
                                    tracing::debug!(error = %e, "SSH proxy connection ended");
                                    return Ok(());
                                }
                            };

                            tracing::info!(
                                bytes_sent,
                                bytes_recv,
                                "Pool manager SSH proxy connection completed"
                            );

                            return Ok(());
                        }
                        // =============================================================
                        // END TEMPORARY
                        // =============================================================

                        let Some(domain) = pg::get_domain(&hostname).await? else {
                            tracing::error!(
                                hostname,
                                "client tried to ssh to supplied domain we can't associate a vm with"
                            );
                            anyhow::bail!("client tried to ssh to supplied domain we can't associate a vm with");
                        };

                        domain.vm_id
                    }
                };

                let Some(vm) = pg::get_vm(&vm_id).await else {
                    tracing::error!(
                        "internal server error, foreign key: domain_id on table vms points to null vm row"
                    );
                    anyhow::bail!("internal server error");
                };

                let wg_start = Instant::now();
                wg.peer_ensure(WgPeer {
                    endpoint_ip: vm.node_ip,
                    port: vm.wg_port,
                    pub_key: vm.wg_public_key,
                    remote_ipv6: vm.vm_ip,
                })?;
                tracing::info!(vm_ip = %vm.vm_ip, elapsed_ms = %wg_start.elapsed().as_millis(), "wg peer_ensure");

                // Connect to VM's SSH port (22)
                let backend_addr = format!("[{}]:22", vm.vm_ip);

                let ssh_connect_start = Instant::now();
                let mut backend_stream = match timeout(
                    Duration::from_secs(VersConfig::proxy().ssh_backend_connect_timeout_secs),
                    tokio::net::TcpStream::connect(&backend_addr),
                )
                .await
                {
                    Ok(Ok(stream)) => {
                        let elapsed = ssh_connect_start.elapsed();
                        tracing::info!(backend = %backend_addr, elapsed_ms = %elapsed.as_millis(), "ssh tcp_connect completed");
                        stream
                    }
                    Ok(Err(e)) => {
                        let elapsed = ssh_connect_start.elapsed();
                        tracing::error!(
                            backend = %backend_addr,
                            elapsed_ms = %elapsed.as_millis(),
                            error = ?e,
                            "ssh tcp_connect failed"
                        );
                        anyhow::bail!("Failed to connect to SSH server: {}", e);
                    }
                    Err(_) => {
                        let elapsed = ssh_connect_start.elapsed();
                        tracing::error!(
                            backend = %backend_addr,
                            elapsed_ms = %elapsed.as_millis(),
                            "ssh tcp_connect timeout"
                        );
                        anyhow::bail!("SSH connection timeout");
                    }
                };

                tracing::info!(
                    backend = %backend_addr,
                    "Connected to SSH server, starting bidirectional forwarding"
                );

                // Forward traffic bidirectionally with idle timeout
                let mut buffered_stream = buffered_stream;
                let ssh_idle_timeout_secs = VersConfig::proxy().ssh_idle_timeout_secs;
                let (bytes_to_backend, bytes_to_client) = if ssh_idle_timeout_secs > 0 {
                    match proxy::idle_copy::copy_bidirectional_with_idle_timeout(
                        &mut buffered_stream,
                        &mut backend_stream,
                        Duration::from_secs(ssh_idle_timeout_secs),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(proxy::idle_copy::CopyIdleError::IdleTimeout) => {
                            tracing::warn!(
                                sni = %sni,
                                idle_timeout_secs = ssh_idle_timeout_secs,
                                "SSH connection idle timeout — no data in either direction"
                            );
                            anyhow::bail!("SSH connection idle timeout");
                        }
                        Err(proxy::idle_copy::CopyIdleError::Io(e)) => {
                            return Err(e.into());
                        }
                    }
                } else {
                    tokio::io::copy_bidirectional(&mut buffered_stream, &mut backend_stream).await?
                };
                tracing::info!(
                    sni = %sni,
                    bytes_to_backend,
                    bytes_to_client,
                    "SSH connection closed"
                );

                // _pool_container_guard drops here, releasing container if it was a pool manager connection
            }
            protocol::Protocol::Tls => {
                tracing::error!(
                    client_addr = %client_addr,
                    "Unexpected TLS protocol after TLS termination - this should not happen"
                );
                anyhow::bail!("Unexpected TLS protocol");
            }
        };

        Ok(())
    }
    .instrument(span)
    .await
}

/// Re-issues a certificate for `domain`, replaces the DB record, and deletes the old cert.
async fn renew_cert_for_domain(
    domain: &str,
    old_cert_id: Uuid,
    acme_client: &AcmeClient,
) -> anyhow::Result<()> {
    tracing::info!(%domain, "starting cert renewal");

    let (mut order, challenges) = acme_client
        .request_certificate_http01(&[domain.to_string()])
        .await
        .context("failed to request cert for renewal")?;

    for challenge in &challenges {
        pg::upsert_acme_http01_challenge(
            &challenge.domain,
            &challenge.token,
            &challenge.key_authorization,
        )
        .await?;
    }

    order
        .notify_ready()
        .await
        .context("notify_ready failed during renewal")?;
    order
        .wait_for_validation()
        .await
        .context("wait_for_validation failed during renewal")?;

    let finalized = order
        .finalize()
        .await
        .context("finalize failed during renewal")?;

    let cert_chain = pem::parse_many(&finalized.certificate_pem)
        .context("failed to parse renewed cert chain")?;
    let cert_private_key =
        pem::parse(&finalized.private_key_pem).context("failed to parse renewed private key")?;

    let not_after = DateTime::from_timestamp(finalized.not_after, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid not_after timestamp in renewed cert"))?;
    let not_before = DateTime::from_timestamp(finalized.not_before, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid not_before timestamp in renewed cert"))?;

    let new_cert_id = Uuid::new_v4();
    pg::insert_cert(
        new_cert_id,
        domain,
        &cert_chain,
        &cert_private_key,
        not_after,
        not_before,
        Utc::now(),
    )
    .await?;
    pg::delete_cert(old_cert_id).await?;
    pg::delete_acme_http01_challenge(domain).await?;

    tracing::info!(%domain, %new_cert_id, %not_after, "cert renewed successfully");
    Ok(())
}

/// Background task that periodically renews custom-domain certs expiring within 30 days.
/// Runs every 12 hours. Skips the system cert (MAGIC_API_VERS_SH_TLS_CERT_ID).
async fn cert_renewal_task(acme_client: AcmeClient) {
    let mut interval = tokio::time::interval(Duration::from_secs(12 * 3600));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        interval.tick().await;

        let domains = match pg::get_domains_needing_renewal(30, MAGIC_API_VERS_SH_TLS_CERT_ID).await
        {
            Ok(d) => d,
            Err(err) => {
                tracing::error!(?err, "cert renewal: failed to query expiring domains");
                continue;
            }
        };

        if domains.is_empty() {
            tracing::info!("cert renewal: no domains need renewal");
            continue;
        }

        tracing::info!(count = domains.len(), "cert renewal: renewing domains");

        for (domain, old_cert_id) in domains {
            if let Err(err) = renew_cert_for_domain(&domain, old_cert_id, &acme_client).await {
                tracing::error!(%domain, ?err, "cert renewal failed for domain");
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls crypto provider (required for rustls 0.23+)
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Initialize tracing subscriber with span close events for timing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .init();

    tracing::info!("Starting proxy server");

    let config = VersConfig::proxy();

    let acme_client = vers_acme::AcmeClient::new(AcmeConfig {
        email: config.acme_email.clone(),
        account_key: config.acme_account_key.clone(),
        directory_url: config.acme_directory_url.clone(),
    })
    .await
    .expect("Failed to init acme client");

    tracing::info!(
        port = VersConfig::proxy().port,
        interface = %VersConfig::proxy().interface,
        admin_interface = %VersConfig::proxy().admin_interface,
        admin_port = VersConfig::proxy().admin_port,
        ssh_port = VersConfig::proxy().ssh_port,
        ssh_cert_path = %VersConfig::proxy().ssh_cert_path.display(),
        ssh_key_path = %VersConfig::proxy().ssh_key_path.display(),
        ssh_tls_handshake_timeout = VersConfig::proxy().ssh_tls_handshake_timeout_secs,
        ssh_backend_connect_timeout = VersConfig::proxy().ssh_backend_connect_timeout_secs,
        ssh_idle_timeout = VersConfig::proxy().ssh_idle_timeout_secs,
        "Proxy configuration loaded"
    );

    tracing::info!("Initializing WireGuard interface");

    let wg = WG::new_with_peers(
        "wgproxy",
        PROXY_PRV_IP.parse().unwrap(),
        VersConfig::proxy().wg_private_key.clone(),
        VersConfig::proxy().wg_port,
        vec![WgPeer {
            pub_key: VersConfig::orchestrator().wg_public_key.clone(),
            endpoint_ip: VersConfig::orchestrator().public_ip.into(),
            remote_ipv6: VersConfig::orchestrator().wg_private_ip,
            port: VersConfig::orchestrator().wg_port,
        }],
    )?;

    tracing::info!("WireGuard interface initialized successfully");

    let admin_addr: SocketAddr = (
        VersConfig::proxy().admin_interface,
        VersConfig::proxy().admin_port,
    )
        .into();
    let admin_listener = TcpListener::bind(&admin_addr).await?;
    tracing::info!(address = %admin_addr, "Admin listener started");

    tracing::info!("Initializing database connection pool");
    pg::init(VersConfig::common().database_url.clone()).await?;
    tracing::info!("Database connection pool initialized successfully");

    tracing::debug!("Initializing metrics collector");
    let metrics = metrics::Metrics::new();

    // Renew custom-domain TLS certs expiring within 30 days
    let acme_client_renewal = acme_client.clone();
    tokio::spawn(async move {
        cert_renewal_task(acme_client_renewal).await;
    });

    // Log metrics every 60 seconds
    let metrics_logger = metrics.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            tracing::info!(
                ssh_total = metrics_logger.ssh_connections_total(),
                ssh_active = metrics_logger.ssh_connections_active(),
                http_total = metrics_logger.http_connections_total(),
                http_active = metrics_logger.http_connections_active(),
                "Metrics summary"
            );
        }
    });

    let admin_router = admin::build_router(
        VersConfig::proxy().admin_api_key.clone(),
        wg.clone(),
        metrics.clone(),
    );
    tokio::spawn(async move {
        if let Err(err) = axum::serve(admin_listener, admin_router).await {
            tracing::error!(error = %err, "Admin server failed");
        }
    });

    let (sender, receiver) = broadcast::channel(1);

    // Set up graceful shutdown signal handler
    tokio::task::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("Received shutdown signal (Ctrl+C), shutting down gracefully");
                let _ = sender.send(());
            }
            Err(err) => {
                tracing::error!(error = ?err, "Error setting up shutdown signal handler");
            }
        }
    });

    // start of here.
    let mut ports_to_bind: Vec<u16> = vec![];

    // Interferes with SNE because postgres: 5432
    ports_to_bind.extend([
        80, 443, 8000, 8080, 9000, 3000, 9999, 5173, 3306, 1337, 3724, 3210,
    ]);

    let (sender, mut recv) = mpsc::channel(ports_to_bind.len());

    for port_to_bind in ports_to_bind.iter() {
        let mut receiver = receiver.resubscribe();
        let shutdown_fut = Box::pin(async move {
            let _ = receiver.recv().await;
        });

        let addr = SocketAddrV4::new(config.interface, *port_to_bind);
        let sender_clone = sender.clone();
        let wg_clone = wg.clone();
        let metrics_clone = metrics.clone();
        let acme_client_clone = acme_client.clone();
        tokio::task::spawn(async move {
            let result = proxy_listener(
                addr.into(),
                shutdown_fut,
                wg_clone,
                metrics_clone,
                acme_client_clone,
            )
            .await;

            if let Err(err) = result {
                tracing::error!(?err, "proxy_listener returned error");
            };

            sender_clone.send(()).await.unwrap();
        });
    }

    tracing::info!("Proxy shutdown complete");

    for _ in 0..ports_to_bind.len() {
        let _ = recv.recv().await.unwrap();
    }
    Ok(())
}

#[tracing::instrument(skip_all, fields(addr = ?addr))]
async fn proxy_listener(
    addr: SocketAddr,
    mut shutdown_future: Pin<Box<dyn Future<Output = ()> + Send>>,
    wg: WG,
    metrics: metrics::Metrics,
    acme_client: AcmeClient,
) -> Result<()> {
    tracing::info!("binding listener");
    let listener = TcpListener::bind(addr).await?;

    loop {
        tokio::select! {
            _ = &mut shutdown_future => {
                tracing::info!("closing listener");
                break;
            }
            accept_result = listener.accept() => {
                let (mut stream, client_addr) = accept_result?;
                // Extract the local port before moving the stream
                let incoming_port = match stream.local_addr() {
                    Ok(addr) => addr.port(),
                    Err(err) => {
                        tracing::warn!(
                            client_addr = %client_addr,
                            error = %err,
                            "Failed to get local address, defaulting to port 80"
                        );
                        80
                    }
                };

                let wg = wg.clone();
                let metrics_clone = metrics.clone();
                let acme_client_clone = acme_client.clone();

                tokio::task::spawn(async move {
                    // Detect protocol
                    match protocol::detect_protocol(&mut stream).await {
                        Ok(protocol::Protocol::Tls) => {
                            // All TLS connections go through handle_tls_connection which
                            // terminates TLS first, then detects the application protocol
                            // (HTTP vs SSH) and routes accordingly
                            tracing::debug!(
                                client_addr = %client_addr,
                                "TLS connection detected, routing to TLS handler"
                            );

                            if let Err(e) = handle_tls_connection(
                                stream,
                                client_addr,
                                &acme_client_clone,
                                &wg,
                                &metrics_clone,
                                incoming_port,
                            ).await {
                                tracing::error!(
                                    client_addr = %client_addr,
                                    error = ?e,
                                    "TLS connection handling failed"
                                );
                            }
                        }
                        Ok(protocol::Protocol::Http) => {
                            let _guard = metrics::ConnectionGuard::new(
                                metrics_clone.clone(),
                                metrics::ConnectionType::Http
                            );

                            tracing::debug!(
                                client_addr = %client_addr,
                                "HTTP connection detected, routing to HTTP handler"
                            );
                            let io = TokioIo::new(stream);
                            let service = service_fn(|req| dispatch(req, client_addr, &wg, &metrics_clone, incoming_port));
                            if let Err(err) = http1::Builder::new()
                                .serve_connection(io, service)
                                .with_upgrades()
                                .await
                            {
                                tracing::error!(
                                    client_addr = %client_addr,
                                    error = ?err,
                                    "Failed to serve HTTP connection"
                                );
                            }
                        }
                        Ok(protocol::Protocol::Ssh) => {
                            tracing::error!(
                                client_addr = %client_addr,
                                "Plain SSH protocol detected (without TLS) - not supported"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                client_addr = %client_addr,
                                error = ?e,
                                "Protocol detection failed"
                            );
                        }
                    }
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod normalize_host_tests {
        use super::*;

        #[test]
        fn test_plain_hostname() {
            assert_eq!(normalize_host("api.vers.sh"), "api.vers.sh");
        }

        #[test]
        fn test_hostname_with_port() {
            assert_eq!(normalize_host("api.vers.sh:8080"), "api.vers.sh");
        }

        #[test]
        fn test_hostname_with_standard_https_port() {
            assert_eq!(normalize_host("api.vers.sh:443"), "api.vers.sh");
        }

        #[test]
        fn test_hostname_with_standard_http_port() {
            assert_eq!(normalize_host("api.vers.sh:80"), "api.vers.sh");
        }

        #[test]
        fn test_vm_hostname() {
            assert_eq!(
                normalize_host("fcf454fa-27ca-4747-ab2c-53f283b9b5d2.vm.vers.sh"),
                "fcf454fa-27ca-4747-ab2c-53f283b9b5d2.vm.vers.sh"
            );
        }

        #[test]
        fn test_vm_hostname_with_port() {
            assert_eq!(
                normalize_host("fcf454fa-27ca-4747-ab2c-53f283b9b5d2.vm.vers.sh:8080"),
                "fcf454fa-27ca-4747-ab2c-53f283b9b5d2.vm.vers.sh"
            );
        }

        #[test]
        fn test_ipv6_literal() {
            assert_eq!(normalize_host("[fd00:fe11:deed::1]"), "fd00:fe11:deed::1");
        }

        #[test]
        fn test_ipv6_literal_with_port() {
            assert_eq!(
                normalize_host("[fd00:fe11:deed::1]:8080"),
                "fd00:fe11:deed::1"
            );
        }

        #[test]
        fn test_ipv4_address() {
            assert_eq!(normalize_host("127.0.0.1"), "127.0.0.1");
        }

        #[test]
        fn test_ipv4_address_with_port() {
            assert_eq!(normalize_host("127.0.0.1:8080"), "127.0.0.1");
        }

        #[test]
        fn test_localhost() {
            assert_eq!(normalize_host("localhost"), "localhost");
        }

        #[test]
        fn test_localhost_with_port() {
            assert_eq!(normalize_host("localhost:3000"), "localhost");
        }

        #[test]
        fn test_empty_string() {
            assert_eq!(normalize_host(""), "");
        }

        #[test]
        fn test_only_port() {
            // Edge case: just ":8080" - should return empty string
            assert_eq!(normalize_host(":8080"), "");
        }

        #[test]
        fn test_malformed_ipv6_no_closing_bracket() {
            // Malformed IPv6 without closing bracket - returns as-is
            assert_eq!(normalize_host("[fd00::1"), "[fd00::1");
        }

        #[test]
        fn test_uppercase_hostname() {
            // Note: normalize_host doesn't change case - case insensitivity
            // is handled by eq_ignore_ascii_case in the routing logic
            assert_eq!(normalize_host("API.VERS.SH"), "API.VERS.SH");
        }

        #[test]
        fn test_uppercase_hostname_with_port() {
            assert_eq!(normalize_host("API.VERS.SH:8080"), "API.VERS.SH");
        }

        #[test]
        fn test_mixed_case_hostname() {
            assert_eq!(normalize_host("Api.Vers.Sh:443"), "Api.Vers.Sh");
        }
    }

    mod streaming_detection_tests {
        /// The proxy exempts streaming exec paths from the forward timeout.
        /// These tests verify the path-matching logic.
        fn is_streaming(path: &str) -> bool {
            path.contains("/exec/stream")
        }

        #[test]
        fn exec_stream_is_streaming() {
            assert!(is_streaming(
                "/api/v1/vm/550e8400-e29b-41d4-a716-446655440000/exec/stream"
            ));
        }

        #[test]
        fn exec_stream_attach_is_streaming() {
            assert!(is_streaming(
                "/api/v1/vm/550e8400-e29b-41d4-a716-446655440000/exec/stream/attach"
            ));
        }

        #[test]
        fn plain_exec_is_not_streaming() {
            assert!(!is_streaming(
                "/api/v1/vm/550e8400-e29b-41d4-a716-446655440000/exec"
            ));
        }

        #[test]
        fn logs_is_not_streaming() {
            assert!(!is_streaming(
                "/api/v1/vm/550e8400-e29b-41d4-a716-446655440000/logs"
            ));
        }

        #[test]
        fn new_root_is_not_streaming() {
            assert!(!is_streaming("/api/v1/vm/new_root"));
        }

        #[test]
        fn status_is_not_streaming() {
            assert!(!is_streaming(
                "/api/v1/vm/550e8400-e29b-41d4-a716-446655440000/status"
            ));
        }
    }
}
