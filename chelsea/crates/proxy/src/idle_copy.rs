//! Bidirectional copy with idle timeout.
//!
//! Drop-in replacement for `tokio::io::copy_bidirectional` that closes the
//! connection when no data has flowed in *either* direction for a given
//! duration.  The standard `tokio::time::timeout` wrapper around
//! `copy_bidirectional` acts as a **total** wall-clock timeout, which is
//! incorrect — it kills active connections that happen to run longer than
//! the limit.

use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Duration, Instant, sleep_until};

/// Error returned when the idle timeout fires.
#[derive(Debug)]
pub struct IdleTimeoutError;

impl std::fmt::Display for IdleTimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "connection idle timeout")
    }
}

impl std::error::Error for IdleTimeoutError {}

/// Bidirectional stream copy that resets an idle timer on every data transfer.
///
/// Returns `(bytes_a_to_b, bytes_b_to_a)` on success, or an error if the
/// connection fails or the idle timeout fires.
///
/// The idle timeout is reset whenever data flows in **either** direction.
pub async fn copy_bidirectional_with_idle_timeout<A, B>(
    a: &mut A,
    b: &mut B,
    idle_timeout: Duration,
) -> Result<(u64, u64), CopyIdleError>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (mut a_read, mut a_write) = tokio::io::split(a);
    let (mut b_read, mut b_write) = tokio::io::split(b);

    let mut a_to_b: u64 = 0;
    let mut b_to_a: u64 = 0;

    let mut a_buf = vec![0u8; 8192];
    let mut b_buf = vec![0u8; 8192];

    let mut deadline = Instant::now() + idle_timeout;

    let mut a_done = false;
    let mut b_done = false;

    loop {
        tokio::select! {
            // a → b direction
            result = a_read.read(&mut a_buf), if !a_done => {
                match result {
                    Ok(0) => {
                        a_done = true;
                        // Shutdown write side of b to signal EOF
                        let _ = b_write.shutdown().await;
                        if b_done {
                            break;
                        }
                    }
                    Ok(n) => {
                        b_write.write_all(&a_buf[..n]).await.map_err(CopyIdleError::Io)?;
                        a_to_b += n as u64;
                        deadline = Instant::now() + idle_timeout;
                    }
                    Err(e) => return Err(CopyIdleError::Io(e)),
                }
            }

            // b → a direction
            result = b_read.read(&mut b_buf), if !b_done => {
                match result {
                    Ok(0) => {
                        b_done = true;
                        // Shutdown write side of a to signal EOF
                        let _ = a_write.shutdown().await;
                        if a_done {
                            break;
                        }
                    }
                    Ok(n) => {
                        a_write.write_all(&b_buf[..n]).await.map_err(CopyIdleError::Io)?;
                        b_to_a += n as u64;
                        deadline = Instant::now() + idle_timeout;
                    }
                    Err(e) => return Err(CopyIdleError::Io(e)),
                }
            }

            // Idle timeout watchdog
            _ = sleep_until(deadline) => {
                return Err(CopyIdleError::IdleTimeout);
            }
        }
    }

    Ok((a_to_b, b_to_a))
}

/// Error type for `copy_bidirectional_with_idle_timeout`.
#[derive(Debug)]
pub enum CopyIdleError {
    Io(io::Error),
    IdleTimeout,
}

impl std::fmt::Display for CopyIdleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CopyIdleError::Io(e) => write!(f, "{}", e),
            CopyIdleError::IdleTimeout => write!(f, "connection idle timeout"),
        }
    }
}

impl std::error::Error for CopyIdleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CopyIdleError::Io(e) => Some(e),
            CopyIdleError::IdleTimeout => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn test_data_flows_both_directions() {
        let (mut client, mut server_end) = duplex(1024);
        let (mut backend, mut backend_end) = duplex(1024);

        let handle = tokio::spawn(async move {
            server_end.write_all(b"hello").await.unwrap();
            backend_end.write_all(b"world").await.unwrap();

            let mut buf = [0u8; 5];
            backend_end.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"hello");

            server_end.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"world");

            drop(server_end);
            drop(backend_end);
        });

        let result =
            copy_bidirectional_with_idle_timeout(&mut client, &mut backend, Duration::from_secs(5))
                .await;

        handle.await.unwrap();

        let (a_to_b, b_to_a) = result.unwrap();
        assert_eq!(a_to_b, 5);
        assert_eq!(b_to_a, 5);
    }

    #[tokio::test]
    async fn test_idle_timeout_fires_when_no_data() {
        let (mut client, _server_end) = duplex(1024);
        let (mut backend, _backend_end) = duplex(1024);

        let result = copy_bidirectional_with_idle_timeout(
            &mut client,
            &mut backend,
            Duration::from_millis(50),
        )
        .await;

        assert!(matches!(result, Err(CopyIdleError::IdleTimeout)));
    }

    #[tokio::test]
    async fn test_activity_resets_timeout() {
        let (mut client, mut server_end) = duplex(1024);
        let (mut backend, mut backend_end) = duplex(1024);

        // Send data every 30ms for 150ms total with a 100ms idle timeout.
        // Without reset, it would fire at 100ms and kill the connection early.
        let handle = tokio::spawn(async move {
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_millis(30)).await;
                server_end.write_all(b"x").await.unwrap();
                let mut buf = [0u8; 1];
                backend_end.read_exact(&mut buf).await.unwrap();
            }
            // Stop sending — idle timeout should fire after 100ms of silence
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(server_end);
            drop(backend_end);
        });

        let result = copy_bidirectional_with_idle_timeout(
            &mut client,
            &mut backend,
            Duration::from_millis(100),
        )
        .await;

        handle.await.unwrap();

        match result {
            Err(CopyIdleError::IdleTimeout) => {
                // Expected — stopped sending and idle timeout fired
            }
            Ok((a_to_b, _)) => {
                // Also acceptable if the drops raced with the timeout
                assert_eq!(a_to_b, 5);
            }
            Err(e) => panic!("unexpected error: {}", e),
        }
    }

    /// Verify large transfers complete without data loss.
    /// This is the key correctness check vs copy_bidirectional — we must
    /// not silently drop bytes in our manual read/write loop.
    #[tokio::test]
    async fn test_large_transfer_no_data_loss() {
        // 1 MB in each direction
        const SIZE: usize = 1024 * 1024;
        // duplex buffer must be large enough to not deadlock when both sides
        // write concurrently; SIZE bytes outstanding in each direction.
        let (mut client, server_end) = duplex(64 * 1024);
        let (mut backend, backend_end) = duplex(64 * 1024);

        let client_payload: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();
        let backend_payload: Vec<u8> = (0..SIZE).map(|i| (i % 239) as u8).collect();

        let expected_from_client = client_payload.clone();
        let expected_from_backend = backend_payload.clone();

        let handle = tokio::spawn(async move {
            // Split streams so we can read and write concurrently without
            // double-mutable-borrow issues.
            let (mut server_read, mut server_write) = tokio::io::split(server_end);
            let (mut backend_read, mut backend_write) = tokio::io::split(backend_end);

            let (_, from_backend, _, from_client) = tokio::join!(
                async {
                    server_write.write_all(&client_payload).await.unwrap();
                    server_write.shutdown().await.unwrap();
                },
                async {
                    let mut received = Vec::new();
                    server_read.read_to_end(&mut received).await.unwrap();
                    received
                },
                async {
                    backend_write.write_all(&backend_payload).await.unwrap();
                    backend_write.shutdown().await.unwrap();
                },
                async {
                    let mut received = Vec::new();
                    backend_read.read_to_end(&mut received).await.unwrap();
                    received
                }
            );

            (from_client, from_backend)
        });

        let result = copy_bidirectional_with_idle_timeout(
            &mut client,
            &mut backend,
            Duration::from_secs(10),
        )
        .await;

        let (from_client, from_backend) = handle.await.unwrap();
        let (a_to_b, b_to_a) = result.unwrap();

        assert_eq!(a_to_b as usize, SIZE);
        assert_eq!(b_to_a as usize, SIZE);
        assert_eq!(from_client, expected_from_client);
        assert_eq!(from_backend, expected_from_backend);
    }

    /// One side closes while the other keeps sending.
    /// Must not panic or lose the data that was already in flight.
    #[tokio::test]
    async fn test_half_close() {
        let (mut client, mut server_end) = duplex(1024);
        let (mut backend, mut backend_end) = duplex(1024);

        let handle = tokio::spawn(async move {
            // Client sends data then closes its write side
            server_end.write_all(b"from-client").await.unwrap();
            server_end.shutdown().await.unwrap();

            // Backend keeps sending after client closed
            backend_end.write_all(b"from-backend").await.unwrap();

            // Read what arrived at backend from client
            let mut buf = vec![0u8; 64];
            let n = backend_end.read(&mut buf).await.unwrap();
            let from_client = &buf[..n];
            assert_eq!(from_client, b"from-client");

            // Read what arrived at server from backend
            let mut buf2 = vec![0u8; 64];
            let n = server_end.read(&mut buf2).await.unwrap();
            let from_backend = &buf2[..n];
            assert_eq!(from_backend, b"from-backend");

            // Now close backend too
            drop(server_end);
            drop(backend_end);
        });

        let result =
            copy_bidirectional_with_idle_timeout(&mut client, &mut backend, Duration::from_secs(5))
                .await;

        handle.await.unwrap();

        let (a_to_b, b_to_a) = result.unwrap();
        assert_eq!(a_to_b, 11); // "from-client"
        assert_eq!(b_to_a, 12); // "from-backend"
    }

    /// Both directions sending concurrently at high throughput.
    /// Verifies we don't deadlock or corrupt data under contention.
    #[tokio::test]
    async fn test_concurrent_bidirectional_traffic() {
        const CHUNKS: usize = 500;
        const CHUNK_SIZE: usize = 1024;

        let (mut client, server_end) = duplex(64 * 1024);
        let (mut backend, backend_end) = duplex(64 * 1024);

        let handle = tokio::spawn(async move {
            let (mut server_read, mut server_write) = tokio::io::split(server_end);
            let (mut backend_read, mut backend_write) = tokio::io::split(backend_end);

            let client_writer = tokio::spawn(async move {
                let chunk = vec![0xAA_u8; CHUNK_SIZE];
                for _ in 0..CHUNKS {
                    server_write.write_all(&chunk).await.unwrap();
                }
                server_write.shutdown().await.unwrap();
            });

            let backend_writer = tokio::spawn(async move {
                let chunk = vec![0xBB_u8; CHUNK_SIZE];
                for _ in 0..CHUNKS {
                    backend_write.write_all(&chunk).await.unwrap();
                }
                backend_write.shutdown().await.unwrap();
            });

            let client_reader = tokio::spawn(async move {
                let mut total = 0usize;
                let mut buf = vec![0u8; 8192];
                loop {
                    let n = server_read.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    // Verify all bytes are 0xBB (from backend)
                    assert!(buf[..n].iter().all(|&b| b == 0xBB));
                    total += n;
                }
                total
            });

            let backend_reader = tokio::spawn(async move {
                let mut total = 0usize;
                let mut buf = vec![0u8; 8192];
                loop {
                    let n = backend_read.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    // Verify all bytes are 0xAA (from client)
                    assert!(buf[..n].iter().all(|&b| b == 0xAA));
                    total += n;
                }
                total
            });

            client_writer.await.unwrap();
            backend_writer.await.unwrap();
            let client_received = client_reader.await.unwrap();
            let backend_received = backend_reader.await.unwrap();

            (backend_received, client_received)
        });

        let result = copy_bidirectional_with_idle_timeout(
            &mut client,
            &mut backend,
            Duration::from_secs(10),
        )
        .await;

        let (backend_received, client_received) = handle.await.unwrap();
        let (a_to_b, b_to_a) = result.unwrap();

        let expected = (CHUNKS * CHUNK_SIZE) as u64;
        assert_eq!(a_to_b, expected);
        assert_eq!(b_to_a, expected);
        assert_eq!(backend_received, expected as usize);
        assert_eq!(client_received, expected as usize);
    }

    /// Idle timeout is disabled (0) — should behave like plain copy_bidirectional.
    /// This tests the caller's codepath where idle_timeout_secs == 0 falls through
    /// to the standard copy_bidirectional, but let's also verify our function
    /// handles a very large timeout gracefully.
    #[tokio::test]
    async fn test_large_timeout_completes_on_stream_close() {
        let (mut client, server_end) = duplex(1024);
        let (mut backend, backend_end) = duplex(1024);

        // Split so we can read/write concurrently on each end without deadlock
        let (mut server_read, mut server_write) = tokio::io::split(server_end);
        let (mut backend_read, mut backend_write) = tokio::io::split(backend_end);

        let handle = tokio::spawn(async move {
            // Both sides write and read concurrently
            let (_, _, _, _) = tokio::join!(
                async {
                    server_write.write_all(b"ping").await.unwrap();
                    server_write.shutdown().await.unwrap();
                },
                async {
                    let mut buf = [0u8; 4];
                    server_read.read_exact(&mut buf).await.unwrap();
                    assert_eq!(&buf, b"pong");
                },
                async {
                    backend_write.write_all(b"pong").await.unwrap();
                    backend_write.shutdown().await.unwrap();
                },
                async {
                    let mut buf = [0u8; 4];
                    backend_read.read_exact(&mut buf).await.unwrap();
                    assert_eq!(&buf, b"ping");
                }
            );
        });

        // Very large timeout — should complete based on stream closure, not timeout
        let result = copy_bidirectional_with_idle_timeout(
            &mut client,
            &mut backend,
            Duration::from_secs(86400),
        )
        .await;

        handle.await.unwrap();

        let (a_to_b, b_to_a) = result.unwrap();
        assert_eq!(a_to_b, 4);
        assert_eq!(b_to_a, 4);
    }
}
