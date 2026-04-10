//! Protocol detection and multiplexing
//!
//! This module provides protocol detection to multiplex HTTP and TLS (SSH-over-TLS)
//! traffic on the same port (typically 443).
//!
//! ## Detection Logic
//!
//! - **TLS**: Starts with TLS ClientHello (0x16 0x03 0x01 or 0x16 0x03 0x03)
//! - **HTTP**: Starts with ASCII text like "GET ", "POST", "PUT ", "HEAD", etc.
//!
//! ## Usage
//!

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::net::TcpStream;

/// Detected protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// TLS connection (encrypted, need to terminate to detect app protocol)
    Tls,
    /// Plain HTTP connection
    Http,
    /// SSH protocol (could be plain or after TLS termination)
    Ssh,
}

/// Peek at the first few bytes of a TCP stream to detect the protocol
///
/// This function uses `peek()` to read without consuming bytes from the stream,
/// allowing the actual handler to process the full connection.
///
/// # Arguments
/// * `stream` - TCP stream to inspect
///
/// # Returns
/// The detected protocol, or an error if detection failed
pub async fn detect_protocol(
    stream: &TcpStream,
) -> std::result::Result<Protocol, DetectProtoError> {
    tracing::trace!("Peeking at stream to detect protocol");
    // Peek at the first 5 bytes without consuming them
    let mut buf = [0u8; 5];
    let n = stream.peek(&mut buf).await?;

    tracing::trace!(bytes_peeked = n, "Peeked bytes from stream");

    let protocol = detect_protocol_from_bytes(&buf, n)?;

    tracing::debug!(protocol = ?protocol, bytes_inspected = n, "Protocol detected");

    Ok(protocol)
}

#[derive(Debug)]
pub enum DetectProtoError {
    NotEnoughBytes,
    ProtocolNotRegonized,
    Io(io::Error),
}

impl From<io::Error> for DetectProtoError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl std::fmt::Display for DetectProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetectProtoError::NotEnoughBytes => write!(f, "Not enough bytes to detect protocol"),
            DetectProtoError::ProtocolNotRegonized => write!(f, "Protocol not recognized"),
            DetectProtoError::Io(err) => write!(f, "IO error during protocol detection: {}", err),
        }
    }
}

impl std::error::Error for DetectProtoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DetectProtoError::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// Core protocol detection logic based on peeked bytes
pub fn detect_protocol_from_bytes(
    buf: &[u8],
    n: usize,
) -> std::result::Result<Protocol, DetectProtoError> {
    if n < 1 {
        tracing::warn!("Connection closed before protocol detection - no bytes available");
        return Err(DetectProtoError::NotEnoughBytes);
    }

    tracing::trace!(
        bytes = n,
        first_bytes = ?&buf[0..n.min(5)],
        "Analyzing bytes for protocol detection"
    );

    // TLS ClientHello detection: 0x16 0x03 XX
    // (0x16 = Handshake, 0x03 = SSL/TLS version major, XX = minor version)
    if n >= 3 && buf[0] == 0x16 && buf[1] == 0x03 {
        tracing::debug!("Detected TLS protocol");
        return Ok(Protocol::Tls);
    }

    // SSH protocol identification: starts with "SSH-" (0x53 0x53 0x48 0x2D)
    // e.g., "SSH-2.0-OpenSSH_8.2p1" or "SSH-1.99-..."
    if n >= 4 && buf.starts_with(b"SSH-") {
        tracing::debug!("Detected SSH protocol");
        return Ok(Protocol::Ssh);
    }

    // HTTP methods start with ASCII letters
    // Common methods: GET, POST, PUT, HEAD, DELETE, OPTIONS, PATCH, CONNECT
    if n >= 3 {
        let prefix = &buf[0..n.min(4)];

        // Check for HTTP methods
        if prefix.starts_with(b"GET ")
            || prefix.starts_with(b"POST")
            || prefix.starts_with(b"PUT ")
            || prefix.starts_with(b"HEAD")
            || prefix.starts_with(b"DELE")
            || prefix.starts_with(b"OPTI")
            || prefix.starts_with(b"PATC")
            || prefix.starts_with(b"CONN")
            || prefix.starts_with(b"TRAC")
        {
            let method = std::str::from_utf8(&buf[0..n.min(4)]);
            assert!(method.is_ok(), "guarranteed by runtime logic");
            tracing::debug!(method = method.unwrap(), "Detected HTTP protocol");
            return Ok(Protocol::Http);
        }
    }

    Err(DetectProtoError::ProtocolNotRegonized)
}

/// Buffered stream that allows detecting protocol after reading initial bytes
///
/// This is used for TLS streams where peek() is not available. We read some bytes,
/// detect the protocol, and then those bytes are replayed when reading from the stream.
pub struct BufferedStream<S> {
    stream: S,
    buffer: Vec<u8>,
    position: usize,
}

impl<S> BufferedStream<S>
where
    S: AsyncRead + Unpin,
{
    /// Create a new BufferedStream by reading initial bytes for protocol detection
    ///
    /// Returns (BufferedStream, Protocol)
    pub async fn new_with_detection(
        mut stream: S,
    ) -> std::result::Result<(Self, Protocol), (DetectProtoError, S)> {
        let mut buf = vec![0u8; 5];
        let n = match stream.read(&mut buf).await {
            Ok(v) => v,
            Err(err) => return Err((err.into(), stream)),
        };

        buf.truncate(n);

        let protocol = match detect_protocol_from_bytes(&buf, n) {
            Ok(v) => v,
            Err(err) => return Err((err, stream)),
        };

        Ok((
            Self {
                stream,
                buffer: buf,
                position: 0,
            },
            protocol,
        ))
    }
}

impl<S> AsyncRead for BufferedStream<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // First, drain any buffered data
        if self.position < self.buffer.len() {
            let remaining = &self.buffer[self.position..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.position += to_copy;
            return Poll::Ready(Ok(()));
        }

        // Buffer is exhausted, read from underlying stream
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl<S> AsyncWrite for BufferedStream<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test TLS 1.0 detection
    #[test]
    fn test_detect_tls_10() {
        let buf = [0x16, 0x03, 0x01, 0x00, 0x05];
        let protocol = detect_protocol_from_bytes(&buf, 5).unwrap();
        assert_eq!(protocol, Protocol::Tls);
    }

    /// Test TLS 1.2 detection
    #[test]
    fn test_detect_tls_12() {
        let buf = [0x16, 0x03, 0x03, 0x00, 0x10];
        let protocol = detect_protocol_from_bytes(&buf, 5).unwrap();
        assert_eq!(protocol, Protocol::Tls);
    }

    /// Test TLS 1.3 detection
    #[test]
    fn test_detect_tls_13() {
        let buf = [0x16, 0x03, 0x04, 0x00, 0x20];
        let protocol = detect_protocol_from_bytes(&buf, 5).unwrap();
        assert_eq!(protocol, Protocol::Tls);
    }

    /// Test HTTP GET detection
    #[test]
    fn test_detect_http_get() {
        let buf = b"GET / HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test HTTP POST detection
    #[test]
    fn test_detect_http_post() {
        let buf = b"POST /api HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test HTTP PUT detection
    #[test]
    fn test_detect_http_put() {
        let buf = b"PUT /resource HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test HTTP HEAD detection
    #[test]
    fn test_detect_http_head() {
        let buf = b"HEAD / HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test HTTP DELETE detection
    #[test]
    fn test_detect_http_delete() {
        let buf = b"DELETE /resource HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test HTTP OPTIONS detection
    #[test]
    fn test_detect_http_options() {
        let buf = b"OPTIONS * HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test HTTP PATCH detection
    #[test]
    fn test_detect_http_patch() {
        let buf = b"PATCH /resource HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test HTTP CONNECT detection
    #[test]
    fn test_detect_http_connect() {
        let buf = b"CONNECT proxy.example.com:443 HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test detection with unknown protocol returns error
    #[test]
    fn test_detect_unknown_returns_error() {
        let buf = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let result = detect_protocol_from_bytes(&buf, 5);
        assert!(result.is_err(), "Unknown protocol should return error");
    }

    /// Test detection with minimal TLS data
    #[test]
    fn test_detect_tls_with_minimal_data() {
        let buf = [0x16, 0x03, 0x03, 0x00, 0x00];
        let protocol = detect_protocol_from_bytes(&buf, 3).unwrap();
        assert_eq!(protocol, Protocol::Tls);
    }

    /// Test HTTP TRACE detection
    #[test]
    fn test_detect_http_trace() {
        let buf = b"TRACE / HTTP/1.1\r\n";
        let protocol = detect_protocol_from_bytes(buf, buf.len()).unwrap();
        assert_eq!(protocol, Protocol::Http);
    }

    /// Test detection with single byte returns error
    #[test]
    fn test_detect_with_no_data() {
        let buf = [0x00; 5];
        let result = detect_protocol_from_bytes(&buf, 0);
        assert!(result.is_err());
    }
}
