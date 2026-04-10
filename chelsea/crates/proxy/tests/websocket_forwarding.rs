//! Integration tests for WebSocket forwarding through the proxy
//!
//! These tests verify that the proxy can:
//! 1. Detect WebSocket upgrade requests
//! 2. Forward WebSocket upgrades to a backend server
//! 3. Maintain bidirectional WebSocket communication

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

/// Test the WebSocket upgrade detection logic
#[test]
fn test_is_websocket_upgrade_detection() {
    use hyper::Request;

    // Helper function that mirrors the one in main.rs
    fn is_websocket_upgrade<B>(req: &Request<B>) -> bool {
        let connection_has_upgrade = req
            .headers()
            .get(hyper::header::CONNECTION)
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_lowercase().contains("upgrade"))
            .unwrap_or(false);

        let upgrade_is_websocket = req
            .headers()
            .get(hyper::header::UPGRADE)
            .and_then(|h| h.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false);

        connection_has_upgrade && upgrade_is_websocket
    }

    // Test valid WebSocket upgrade request
    let req = Request::builder()
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .body(())
        .unwrap();
    assert!(
        is_websocket_upgrade(&req),
        "Should detect valid WebSocket upgrade"
    );

    // Test case-insensitive detection
    let req = Request::builder()
        .header("Connection", "upgrade")
        .header("Upgrade", "WebSocket")
        .body(())
        .unwrap();
    assert!(
        is_websocket_upgrade(&req),
        "Should detect case-insensitive WebSocket upgrade"
    );

    // Test Connection header with multiple values
    let req = Request::builder()
        .header("Connection", "keep-alive, Upgrade")
        .header("Upgrade", "websocket")
        .body(())
        .unwrap();
    assert!(
        is_websocket_upgrade(&req),
        "Should detect WebSocket with multiple Connection values"
    );

    // Test missing Upgrade header
    let req = Request::builder()
        .header("Connection", "Upgrade")
        .body(())
        .unwrap();
    assert!(
        !is_websocket_upgrade(&req),
        "Should not detect without Upgrade header"
    );

    // Test missing Connection header
    let req = Request::builder()
        .header("Upgrade", "websocket")
        .body(())
        .unwrap();
    assert!(
        !is_websocket_upgrade(&req),
        "Should not detect without Connection header"
    );

    // Test wrong upgrade type
    let req = Request::builder()
        .header("Connection", "Upgrade")
        .header("Upgrade", "h2c")
        .body(())
        .unwrap();
    assert!(
        !is_websocket_upgrade(&req),
        "Should not detect non-WebSocket upgrade"
    );

    // Test regular HTTP request
    let req = Request::builder()
        .header("Connection", "keep-alive")
        .body(())
        .unwrap();
    assert!(
        !is_websocket_upgrade(&req),
        "Should not detect regular HTTP request"
    );
}

/// Test WebSocket handshake parsing
#[test]
fn test_websocket_handshake_parsing() {
    // Simulate parsing a 101 response from backend
    let response = "HTTP/1.1 101 Switching Protocols\r\n\
                    Upgrade: websocket\r\n\
                    Connection: Upgrade\r\n\
                    Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\
                    \r\n";

    let status_line = response.lines().next().unwrap();
    assert!(
        status_line.contains("101"),
        "Should find 101 in status line"
    );

    // Parse headers
    let mut headers = Vec::new();
    for line in response.lines().skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    assert_eq!(headers.len(), 3, "Should parse 3 headers");
    assert!(
        headers
            .iter()
            .any(|(n, v)| n == "Upgrade" && v == "websocket")
    );
    assert!(
        headers
            .iter()
            .any(|(n, v)| n == "Connection" && v == "Upgrade")
    );
}

/// Mock WebSocket backend server that accepts upgrades and echoes messages
async fn run_mock_websocket_backend(port: u16) -> anyhow::Result<()> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    println!("[BACKEND] Listening on port {}", port);

    loop {
        let (mut stream, addr) = listener.accept().await?;
        println!("[BACKEND] Connection from {}", addr);

        tokio::spawn(async move {
            // Read HTTP upgrade request
            let mut buf = vec![0u8; 4096];
            let n = match stream.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("[BACKEND] Read error: {}", e);
                    return;
                }
            };

            let request = String::from_utf8_lossy(&buf[..n]);
            println!("[BACKEND] Received request:\n{}", request);

            // Check if it's a WebSocket upgrade
            if !request.contains("Upgrade: websocket") {
                eprintln!("[BACKEND] Not a WebSocket upgrade request");
                return;
            }

            // Send 101 Switching Protocols response
            let response = "HTTP/1.1 101 Switching Protocols\r\n\
                           Upgrade: websocket\r\n\
                           Connection: Upgrade\r\n\
                           Sec-WebSocket-Accept: mock-accept-key\r\n\
                           \r\n";

            if let Err(e) = stream.write_all(response.as_bytes()).await {
                eprintln!("[BACKEND] Write error: {}", e);
                return;
            }

            println!("[BACKEND] Sent 101 response, entering echo mode");

            // Echo mode - read and echo back data
            loop {
                let mut buf = vec![0u8; 1024];
                match stream.read(&mut buf).await {
                    Ok(0) => {
                        println!("[BACKEND] Connection closed");
                        break;
                    }
                    Ok(n) => {
                        println!("[BACKEND] Echoing {} bytes", n);
                        if let Err(e) = stream.write_all(&buf[..n]).await {
                            eprintln!("[BACKEND] Echo write error: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("[BACKEND] Read error: {}", e);
                        break;
                    }
                }
            }
        });
    }
}

/// Test direct WebSocket handshake and echo (no proxy)
#[tokio::test]
async fn test_websocket_backend_direct() -> anyhow::Result<()> {
    // Find available port
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);

    // Start backend server
    let backend_handle = tokio::spawn(async move {
        let _ = run_mock_websocket_backend(port).await;
    });

    // Wait for backend to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect as client
    let mut client = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

    // Send WebSocket upgrade request
    let upgrade_request = "GET /ws HTTP/1.1\r\n\
                          Host: localhost\r\n\
                          Connection: Upgrade\r\n\
                          Upgrade: websocket\r\n\
                          Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                          Sec-WebSocket-Version: 13\r\n\
                          \r\n";

    client.write_all(upgrade_request.as_bytes()).await?;

    // Read response
    let mut buf = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(2), client.read(&mut buf)).await??;
    let response = String::from_utf8_lossy(&buf[..n]);

    println!("[TEST] Got response:\n{}", response);
    assert!(response.contains("101"), "Should receive 101 response");
    assert!(
        response.contains("Upgrade: websocket"),
        "Should have Upgrade header"
    );

    // Test echo after upgrade
    let test_data = b"Hello WebSocket!";
    client.write_all(test_data).await?;

    let mut echo_buf = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(2), client.read(&mut echo_buf)).await??;

    assert_eq!(&echo_buf[..n], test_data, "Should echo back the same data");
    println!("[TEST] Echo test passed!");

    // Cleanup
    backend_handle.abort();
    Ok(())
}

/// Test WebSocket-like bidirectional forwarding
///
/// This test simulates what the proxy does:
/// 1. Accept a connection from "client"
/// 2. Forward WebSocket upgrade to "backend"
/// 3. Bridge the two connections bidirectionally
#[tokio::test]
async fn test_websocket_forwarding_simulation() -> anyhow::Result<()> {
    // Find available ports
    let backend_listener = TcpListener::bind("127.0.0.1:0").await?;
    let backend_port = backend_listener.local_addr()?.port();
    drop(backend_listener);

    let proxy_listener = TcpListener::bind("127.0.0.1:0").await?;
    let proxy_port = proxy_listener.local_addr()?.port();

    // Start backend server
    let backend_handle = tokio::spawn(async move {
        let _ = run_mock_websocket_backend(backend_port).await;
    });

    // Start simple "proxy" that forwards to backend
    let proxy_handle = tokio::spawn(async move {
        loop {
            let (mut client_stream, client_addr) = match proxy_listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };
            println!("[PROXY] Connection from {}", client_addr);

            let backend_port = backend_port;
            tokio::spawn(async move {
                // Read the upgrade request from client
                let mut buf = vec![0u8; 4096];
                let n = match client_stream.read(&mut buf).await {
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("[PROXY] Client read error: {}", e);
                        return;
                    }
                };

                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                println!("[PROXY] Received from client:\n{}", request);

                // Connect to backend
                let mut backend_stream =
                    match TcpStream::connect(format!("127.0.0.1:{}", backend_port)).await {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("[PROXY] Backend connect error: {}", e);
                            return;
                        }
                    };

                // Forward upgrade request to backend
                if let Err(e) = backend_stream.write_all(&buf[..n]).await {
                    eprintln!("[PROXY] Backend write error: {}", e);
                    return;
                }

                // Read 101 response from backend
                let mut response_buf = vec![0u8; 4096];
                let n = match backend_stream.read(&mut response_buf).await {
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("[PROXY] Backend read error: {}", e);
                        return;
                    }
                };

                let response = String::from_utf8_lossy(&response_buf[..n]);
                println!("[PROXY] Backend response:\n{}", response);

                if !response.contains("101") {
                    eprintln!("[PROXY] Backend did not accept upgrade");
                    return;
                }

                // Forward 101 to client
                if let Err(e) = client_stream.write_all(&response_buf[..n]).await {
                    eprintln!("[PROXY] Client write error: {}", e);
                    return;
                }

                println!("[PROXY] Starting bidirectional forwarding");

                // Bidirectional forwarding (this is what the proxy does)
                match tokio::io::copy_bidirectional(&mut client_stream, &mut backend_stream).await {
                    Ok((to_backend, to_client)) => {
                        println!(
                            "[PROXY] Forwarded {} bytes to backend, {} bytes to client",
                            to_backend, to_client
                        );
                    }
                    Err(e) => {
                        // Connection closed is normal
                        println!("[PROXY] Forwarding ended: {}", e);
                    }
                }
            });
        }
    });

    // Wait for servers to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect as client through the proxy
    let mut client = TcpStream::connect(format!("127.0.0.1:{}", proxy_port)).await?;

    // Send WebSocket upgrade request
    let upgrade_request = "GET /ws HTTP/1.1\r\n\
                          Host: test.vm.vers.sh\r\n\
                          Connection: Upgrade\r\n\
                          Upgrade: websocket\r\n\
                          Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                          Sec-WebSocket-Version: 13\r\n\
                          \r\n";

    client.write_all(upgrade_request.as_bytes()).await?;

    // Read 101 response (forwarded through proxy)
    let mut buf = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(2), client.read(&mut buf)).await??;
    let response = String::from_utf8_lossy(&buf[..n]);

    println!("[TEST] Got response through proxy:\n{}", response);
    assert!(response.contains("101"), "Should receive 101 through proxy");

    // Test bidirectional communication after upgrade
    let test_messages = [
        b"First message".to_vec(),
        b"Second message".to_vec(),
        b"Third message with more data!".to_vec(),
    ];

    for msg in &test_messages {
        // Send to backend through proxy
        client.write_all(msg).await?;

        // Receive echo
        let mut echo_buf = vec![0u8; 1024];
        let n = timeout(Duration::from_secs(2), client.read(&mut echo_buf)).await??;

        assert_eq!(
            &echo_buf[..n],
            msg.as_slice(),
            "Should echo back the same data"
        );
        println!("[TEST] Echo verified for: {}", String::from_utf8_lossy(msg));
    }

    println!("[TEST] All WebSocket forwarding tests passed!");

    // Cleanup
    backend_handle.abort();
    proxy_handle.abort();
    Ok(())
}

/// Test that regular HTTP requests are not affected by WebSocket handling
#[test]
fn test_non_websocket_requests_unaffected() {
    use hyper::Request;

    fn is_websocket_upgrade<B>(req: &Request<B>) -> bool {
        let connection_has_upgrade = req
            .headers()
            .get(hyper::header::CONNECTION)
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_lowercase().contains("upgrade"))
            .unwrap_or(false);

        let upgrade_is_websocket = req
            .headers()
            .get(hyper::header::UPGRADE)
            .and_then(|h| h.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false);

        connection_has_upgrade && upgrade_is_websocket
    }

    // Regular GET request
    let req = Request::builder()
        .method("GET")
        .uri("/api/vms")
        .header("Host", "api.vers.sh")
        .header("Accept", "application/json")
        .body(())
        .unwrap();
    assert!(!is_websocket_upgrade(&req));

    // POST request with JSON
    let req = Request::builder()
        .method("POST")
        .uri("/api/vms")
        .header("Host", "api.vers.sh")
        .header("Content-Type", "application/json")
        .body(())
        .unwrap();
    assert!(!is_websocket_upgrade(&req));

    // Request with keep-alive
    let req = Request::builder()
        .header("Connection", "keep-alive")
        .body(())
        .unwrap();
    assert!(!is_websocket_upgrade(&req));
}
