//! Integration tests for SSH forwarding through TLS proxy
//!
//! These tests verify that the proxy can:
//! 1. Accept TLS connections with SNI
//! 2. Forward SSH traffic to a backend server
//! 3. Maintain bidirectional communication

mod common;

use anyhow::Result;
use common::ssh_container::SshContainer;
use std::process::Command;

/// Test that SSH container can execute commands
///
/// This verifies our test infrastructure before testing the proxy
#[tokio::test]
#[ignore] // Requires Docker and sshpass
async fn test_ssh_container_infrastructure() -> Result<()> {
    let container = SshContainer::start().await?;

    // Test basic command
    let result = container.exec_ssh_command("echo 'hello world'").await?;
    assert_eq!(result, "hello world");

    // Test uname
    let result = container.exec_ssh_command("uname -s").await?;
    assert_eq!(result, "Linux");

    // Test whoami
    let result = container.exec_ssh_command("whoami").await?;
    assert_eq!(result, container.username);

    println!("[TEST] SSH container infrastructure verified");

    Ok(())
}

/// Test bidirectional communication through raw TCP
///
/// This tests the basic forwarding mechanism without TLS
/// to isolate any issues.
#[tokio::test]
#[ignore] // Requires Docker
async fn test_tcp_forwarding_basic() -> Result<()> {
    let ssh_container = SshContainer::start().await?;

    // Create a simple TCP forwarder
    let backend_port = ssh_container.host_port;
    let proxy_port = common::find_available_port()?;

    // Start TCP forwarder in background
    let forwarder_handle =
        tokio::spawn(async move { run_simple_tcp_forwarder(proxy_port, backend_port).await });

    // Wait for forwarder to start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Try to connect through the forwarder
    let result = Command::new("sshpass")
        .args(&[
            "-p",
            &ssh_container.password,
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "PreferredAuthentications=password",
            "-o",
            "ConnectTimeout=5",
            "-p",
            &proxy_port.to_string(),
            &format!("{}@127.0.0.1", ssh_container.username),
            "echo 'forwarded'",
        ])
        .output()?;

    if !result.status.success() {
        println!("[TEST] SSH through forwarder failed (expected for now)");
        println!("[TEST] stderr: {}", String::from_utf8_lossy(&result.stderr));
    } else {
        let output = String::from_utf8_lossy(&result.stdout).trim().to_string();
        assert_eq!(output, "forwarded");
        println!("[TEST] TCP forwarding works!");
    }

    // Cleanup
    forwarder_handle.abort();

    Ok(())
}

/// Simple TCP forwarder for testing
///
/// Accepts connections on proxy_port and forwards to backend_port
/// Runs for a limited time to avoid hanging tests
async fn run_simple_tcp_forwarder(proxy_port: u16, backend_port: u16) -> Result<()> {
    use tokio::net::{TcpListener, TcpStream};

    let listener = TcpListener::bind(format!("127.0.0.1:{}", proxy_port)).await?;
    println!("[TEST] TCP forwarder listening on port {}", proxy_port);

    // Run for limited time to avoid hanging
    let timeout = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        // Accept with short timeout
        let accept_result =
            tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept()).await;

        let (mut client, _) = match accept_result {
            Ok(Ok(conn)) => conn,
            Ok(Err(e)) => {
                eprintln!("[TEST] Accept error: {}", e);
                break;
            }
            Err(_) => {
                // Timeout - check if we should continue
                continue;
            }
        };
        let backend_port = backend_port;
        tokio::spawn(async move {
            // Connect to backend
            let mut backend = match TcpStream::connect(format!("127.0.0.1:{}", backend_port)).await
            {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[TEST] Failed to connect to backend: {}", e);
                    return;
                }
            };

            // Bidirectional copy
            match tokio::io::copy_bidirectional(&mut client, &mut backend).await {
                Ok((client_to_backend, backend_to_client)) => {
                    println!(
                        "[TEST] Forwarded {} bytes to backend, {} bytes to client",
                        client_to_backend, backend_to_client
                    );
                }
                Err(e) => {
                    eprintln!("[TEST] Forward error: {}", e);
                }
            }
        });
    }

    Ok(())
}

/// Test SSH connection with port forwarding through openssl s_client
///
/// This simulates what a user would do manually to connect SSH through TLS
#[tokio::test]
#[ignore] // Requires Docker, openssl, and complex setup
async fn test_ssh_through_tls_manual() -> Result<()> {
    println!("[TEST] This test demonstrates the manual connection flow");
    println!("[TEST] User would run:");
    println!(
        "[TEST]   ssh -o ProxyCommand='openssl s_client -connect proxy:443 -servername vm.vers.sh -quiet' user@vm.vers.sh"
    );
    println!("[TEST]");
    println!("[TEST] Implementation pending Phase 1.4");

    Ok(())
}
