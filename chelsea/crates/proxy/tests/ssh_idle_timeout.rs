//! Integration tests for SSH idle timeout behavior.
//!
//! Uses testcontainers to spin up a real SSH server, then connects through
//! a minimal TLS forwarder that uses `copy_bidirectional_with_idle_timeout`.
//! This verifies that real SSH protocol traffic (auth handshake, command
//! execution, interactive sessions) works correctly through our idle-aware
//! copy loop.

mod common;

use anyhow::Result;
use common::ssh_container::SshContainer;
use proxy::idle_copy::{CopyIdleError, copy_bidirectional_with_idle_timeout};
use std::process::Command;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Duration;

/// Start a TCP forwarder that uses our idle-aware copy function.
///
/// Binds to an available port, signals readiness via the oneshot channel,
/// accepts one connection, then forwards traffic with the given idle timeout.
async fn start_idle_forwarder(
    backend_port: u16,
    idle_timeout: Duration,
) -> (u16, tokio::task::JoinHandle<Result<ForwardResult>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listen_port = listener.local_addr().unwrap().port();

    let handle = tokio::spawn(async move {
        let (mut client, _) = listener.accept().await?;
        let mut backend = TcpStream::connect(format!("127.0.0.1:{}", backend_port)).await?;

        match copy_bidirectional_with_idle_timeout(&mut client, &mut backend, idle_timeout).await {
            Ok((a_to_b, b_to_a)) => Ok(ForwardResult {
                bytes_to_backend: a_to_b,
                bytes_to_client: b_to_a,
                idle_timeout: false,
            }),
            Err(CopyIdleError::IdleTimeout) => Ok(ForwardResult {
                bytes_to_backend: 0,
                bytes_to_client: 0,
                idle_timeout: true,
            }),
            Err(CopyIdleError::Io(e)) => Err(e.into()),
        }
    });

    (listen_port, handle)
}

struct ForwardResult {
    bytes_to_backend: u64,
    bytes_to_client: u64,
    idle_timeout: bool,
}

/// Run an SSH command through the forwarder using sshpass.
/// Uses spawn_blocking so we don't block the tokio executor (which the
/// forwarder task also needs to run on).
async fn ssh_through_forwarder(
    port: u16,
    username: &str,
    password: &str,
    command: &str,
) -> Result<String> {
    let port = port;
    let username = username.to_string();
    let password = password.to_string();
    let command = command.to_string();

    tokio::task::spawn_blocking(move || {
        let output = Command::new("sshpass")
            .args([
                "-p",
                &password,
                "ssh",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "PreferredAuthentications=password",
                "-o",
                "ConnectTimeout=10",
                "-p",
                &port.to_string(),
                &format!("{}@127.0.0.1", username),
                &command,
            ])
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "SSH command failed (exit {}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    })
    .await?
}

/// Real SSH session completing a command through the idle-aware forwarder.
/// Verifies that the SSH handshake + command execution + clean shutdown
/// all work through our chunked read/write loop.
#[tokio::test]
#[ignore] // Requires Docker and sshpass
async fn test_ssh_command_through_idle_forwarder() -> Result<()> {
    let container = SshContainer::start().await?;

    let (proxy_port, forwarder) = start_idle_forwarder(
        container.host_port,
        Duration::from_secs(30), // generous timeout — should not fire
    )
    .await;

    let result = ssh_through_forwarder(
        proxy_port,
        &container.username,
        &container.password,
        "echo 'hello from idle forwarder'",
    )
    .await?;

    assert_eq!(result, "hello from idle forwarder");

    let forward_result = forwarder.await??;
    assert!(!forward_result.idle_timeout);
    assert!(forward_result.bytes_to_backend > 0);
    assert!(forward_result.bytes_to_client > 0);

    println!(
        "[TEST] SSH command succeeded: {} bytes to backend, {} bytes to client",
        forward_result.bytes_to_backend, forward_result.bytes_to_client
    );

    Ok(())
}

/// Multiple commands in sequence through the same forwarder setup.
/// Each command is a separate SSH connection, proving the forwarder
/// handles clean connection teardown correctly.
#[tokio::test]
#[ignore] // Requires Docker and sshpass
async fn test_multiple_ssh_commands() -> Result<()> {
    let container = SshContainer::start().await?;

    for cmd in &["uname -s", "whoami", "echo test123", "cat /etc/hostname"] {
        let (proxy_port, forwarder) =
            start_idle_forwarder(container.host_port, Duration::from_secs(30)).await;

        let result =
            ssh_through_forwarder(proxy_port, &container.username, &container.password, cmd).await;

        // We just care that it doesn't hang or corrupt — some commands
        // might have different outputs across containers
        assert!(
            result.is_ok(),
            "Command '{}' failed: {:?}",
            cmd,
            result.err()
        );

        let forward_result = forwarder.await??;
        assert!(
            !forward_result.idle_timeout,
            "Command '{}' hit idle timeout",
            cmd
        );
    }

    println!("[TEST] All SSH commands succeeded through idle forwarder");
    Ok(())
}

/// Large data transfer through the forwarder.
/// Generates a big payload on the SSH server and reads it back, verifying
/// no data corruption in our manual read/write loop.
#[tokio::test]
#[ignore] // Requires Docker and sshpass
async fn test_large_transfer_through_ssh() -> Result<()> {
    let container = SshContainer::start().await?;

    let (proxy_port, forwarder) =
        start_idle_forwarder(container.host_port, Duration::from_secs(30)).await;

    // Generate 100KB of deterministic data on the server and pipe it back
    let result = ssh_through_forwarder(
        proxy_port,
        &container.username,
        &container.password,
        "dd if=/dev/urandom bs=1024 count=100 2>/dev/null | base64",
    )
    .await?;

    // base64 of 100KB ≈ 137KB of text
    assert!(
        result.len() > 100_000,
        "Expected >100KB of output, got {} bytes",
        result.len()
    );

    let forward_result = forwarder.await??;
    assert!(!forward_result.idle_timeout);
    assert!(forward_result.bytes_to_client > 100_000);

    println!(
        "[TEST] Large transfer succeeded: {} bytes through forwarder",
        forward_result.bytes_to_client
    );

    Ok(())
}

/// Verify that an idle connection actually gets killed.
/// Opens an SSH connection that does nothing, and confirms the forwarder
/// terminates it after the idle timeout.
#[tokio::test]
#[ignore] // Requires Docker and sshpass
async fn test_idle_connection_gets_killed() -> Result<()> {
    let container = SshContainer::start().await?;

    // Very short idle timeout
    let (proxy_port, forwarder) =
        start_idle_forwarder(container.host_port, Duration::from_millis(500)).await;

    // Start an SSH session that sleeps longer than the idle timeout.
    // The forwarder should kill the connection while it's sleeping.
    let ssh_handle = tokio::task::spawn_blocking(move || {
        Command::new("sshpass")
            .args([
                "-p",
                "testpass",
                "ssh",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "PreferredAuthentications=password",
                "-o",
                "ConnectTimeout=10",
                "-p",
                &proxy_port.to_string(),
                "testuser@127.0.0.1",
                "sleep 30",
            ])
            .output()
    });

    let forward_result = forwarder.await??;
    assert!(
        forward_result.idle_timeout,
        "Expected idle timeout to fire, but connection closed normally"
    );

    // SSH process should have been killed by the broken pipe
    let ssh_output = ssh_handle.await??;
    assert!(
        !ssh_output.status.success(),
        "SSH should have failed due to broken connection"
    );

    println!("[TEST] Idle connection correctly killed after timeout");

    Ok(())
}

/// Active SSH session that produces output periodically should NOT be killed
/// by the idle timeout, even if the total duration exceeds it.
/// This is the key regression test — the old wall-clock timeout would kill this.
#[tokio::test]
#[ignore] // Requires Docker and sshpass
async fn test_active_session_survives_past_idle_timeout_duration() -> Result<()> {
    let container = SshContainer::start().await?;

    // 500ms idle timeout, but the command runs for ~1.5s total
    // producing output every 300ms — should never be idle long enough to trigger
    let (proxy_port, forwarder) =
        start_idle_forwarder(container.host_port, Duration::from_millis(500)).await;

    // Print something every 300ms for 5 iterations = 1.5s total
    // With a 500ms idle timeout, the old wall-clock approach would kill at 500ms
    let result = ssh_through_forwarder(
        proxy_port,
        &container.username,
        &container.password,
        "for i in 1 2 3 4 5; do echo \"tick $i\"; sleep 0.3; done",
    )
    .await?;

    assert!(result.contains("tick 1"));
    assert!(result.contains("tick 5"));

    let forward_result = forwarder.await??;
    assert!(
        !forward_result.idle_timeout,
        "Active session should NOT have been killed by idle timeout"
    );

    println!(
        "[TEST] Active session survived past idle timeout duration: {}",
        result
    );

    Ok(())
}
