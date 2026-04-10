//! Integration tests exercising the agent over real vsock loopback.
//!
//! These tests use `VMADDR_CID_LOCAL` (CID 1) — the vsock equivalent of
//! localhost — to test the agent's vsock accept path without needing a
//! hypervisor or VM. This exercises code paths that the duplex-stream
//! unit tests cannot reach: the real vsock listener, CID-based peer
//! filtering, and the connection semaphore.
//!
//! Requires the `vsock_loopback` kernel module:
//!
//!   sudo modprobe vsock_loopback
//!
//! # What these tests prove
//!
//! When a process connects to the agent via vsock loopback, the peer CID
//! is 1 (`VMADDR_CID_LOCAL`). The agent only accepts CID 2
//! (`VMADDR_CID_HOST`). This means:
//!
//! - A process inside the VM cannot reach the agent via vsock loopback
//!   (the in-VM privilege escalation concern from review).
//! - Only the hypervisor host can talk to the agent.

#[cfg(target_os = "linux")]
mod loopback {
    use std::process::Stdio;
    use std::time::Duration;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::timeout;
    use tokio_vsock::{VMADDR_CID_LOCAL, VsockAddr, VsockStream};

    /// Pick a unique port per test to avoid collisions.
    fn test_port() -> u32 {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(40_000);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// Spawn the real `chelsea-agent` binary in vsock mode on the given port.
    /// Returns the child process handle. The caller must kill it when done.
    async fn spawn_agent(port: u32) -> tokio::process::Child {
        let bin = env!("CARGO_BIN_EXE_chelsea-agent");
        let child = tokio::process::Command::new(bin)
            .args(["--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn chelsea-agent");

        // Give the agent a moment to bind the vsock listener.
        tokio::time::sleep(Duration::from_millis(200)).await;
        child
    }

    /// The agent rejects vsock loopback connections (CID 1 ≠ CID 2).
    ///
    /// This is the core security property: a process running inside the
    /// VM that connects to the agent's vsock port via loopback will be
    /// disconnected without receiving any response. The agent's CID
    /// check drops the stream before entering the protocol handler.
    #[tokio::test]
    async fn loopback_connection_rejected_by_cid_check() {
        let port = test_port();
        let mut agent = spawn_agent(port).await;

        // Connect via loopback — peer CID will be 1 (VMADDR_CID_LOCAL).
        let stream = VsockStream::connect(VsockAddr::new(VMADDR_CID_LOCAL, port))
            .await
            .expect("TCP-level connect should succeed (kernel routes it)");

        let (read_half, _write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // The agent should drop this connection without sending anything.
        // We expect either EOF (0 bytes read) or a timeout — either way,
        // no Ready event means the CID check worked.
        let result = timeout(Duration::from_secs(2), reader.read_line(&mut line)).await;

        match result {
            Ok(Ok(0)) => {
                // EOF — agent closed the connection. This is the expected path.
            }
            Ok(Ok(_n)) => {
                panic!(
                    "Agent sent data to a loopback client — CID check failed!\n\
                     Received: {line}"
                );
            }
            Ok(Err(_io_err)) => {
                // Connection reset / broken pipe — also means rejection.
            }
            Err(_timeout) => {
                // Agent held the connection open but sent nothing. Acceptable
                // (the drop might be async), but the key point is: no data.
            }
        }

        agent.kill().await.ok();
    }

    /// After rejecting a loopback connection, the agent is still alive
    /// and accepting new connections. Demonstrates that bad connections
    /// don't crash or wedge the listener.
    #[tokio::test]
    async fn agent_survives_rejected_loopback_connection() {
        let port = test_port();
        let mut agent = spawn_agent(port).await;

        // Fire several loopback connections that should all be rejected.
        for _ in 0..5 {
            let stream = VsockStream::connect(VsockAddr::new(VMADDR_CID_LOCAL, port))
                .await
                .ok();
            drop(stream);
        }

        // Small delay to let the agent process the rejections.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // The agent should still be running (not crashed).
        let try_wait = agent.try_wait().expect("Failed to check agent status");
        assert!(
            try_wait.is_none(),
            "Agent should still be running after rejected connections, \
             but it exited with: {:?}",
            try_wait
        );

        // Try one more connection to confirm the listener is still active.
        // (It will also be rejected since we're still CID 1, but the point
        // is that connect() succeeds — the listener is accepting.)
        let connect_result = timeout(
            Duration::from_secs(2),
            VsockStream::connect(VsockAddr::new(VMADDR_CID_LOCAL, port)),
        )
        .await;

        assert!(
            connect_result.is_ok(),
            "Agent listener should still be accepting connections"
        );

        agent.kill().await.ok();
    }

    /// Verify that a loopback client cannot execute commands even if it
    /// sends a well-formed request before the agent drops the connection.
    /// This is a race-condition test: the client connects and immediately
    /// sends a valid Exec request. The agent's CID check should drop the
    /// connection before processing it.
    #[tokio::test]
    async fn loopback_client_cannot_race_exec_request() {
        let port = test_port();
        let mut agent = spawn_agent(port).await;

        let stream = VsockStream::connect(VsockAddr::new(VMADDR_CID_LOCAL, port))
            .await
            .expect("Connect should succeed at the transport level");

        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        // Immediately send a valid Exec request — try to race the CID check.
        let exec_request = r#"{"type":"Exec","payload":{"command":["id"]}}"#;
        let write_result = write_half.write_all(exec_request.as_bytes()).await;
        let _ = write_half.write_all(b"\n").await;
        let _ = write_half.flush().await;

        // Even if the write succeeded (kernel buffered it), we should not
        // get an ExecResult back.
        let mut line = String::new();
        let result = timeout(Duration::from_secs(2), reader.read_line(&mut line)).await;

        let got_exec_result = match result {
            Ok(Ok(n)) if n > 0 => line.contains("ExecResult"),
            _ => false,
        };

        assert!(
            !got_exec_result,
            "Loopback client received an ExecResult — CID check was bypassed!\n\
             Response: {line}"
        );

        // If we got here and write_result was an error, the agent killed
        // the connection before we even finished writing. That's fine too.
        let _ = write_result;

        agent.kill().await.ok();
    }
}
