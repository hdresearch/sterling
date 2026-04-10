//! Common test utilities for integration tests
//!
//! This module provides helpers for:
//! - Starting test SSH proxy servers
//! - Creating test TLS clients
//! - Capturing logs and SNI
//! - Managing test certificates
//! - Running SSH server containers for integration testing

#![allow(dead_code)]

pub mod ssh_container;

use anyhow::Result;
use std::net::TcpStream as StdTcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

/// Captured SNI hostname during test
#[derive(Debug, Clone, Default)]
pub struct CapturedSni {
    inner: Arc<Mutex<Option<String>>>,
}

impl CapturedSni {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set(&self, hostname: String) {
        let mut inner = self.inner.lock().unwrap();
        *inner = Some(hostname);
    }

    pub fn get(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        inner.clone()
    }

    pub fn wait_for_sni(&self, timeout_ms: u64) -> Option<String> {
        let start = std::time::Instant::now();
        loop {
            if let Some(sni) = self.get() {
                return Some(sni);
            }
            if start.elapsed() > Duration::from_millis(timeout_ms) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Test server configuration
pub struct TestServer {
    pub port: u16,
    pub cert_path: String,
    pub key_path: String,
    pub captured_sni: CapturedSni,
}

impl TestServer {
    /// Create a new test server configuration with temp certs
    pub fn new() -> Result<Self> {
        let port = find_available_port()?;
        let temp_dir = std::env::temp_dir();
        let test_id = uuid::Uuid::new_v4();

        Ok(Self {
            port,
            cert_path: format!("{}/test-cert-{}.pem", temp_dir.display(), test_id),
            key_path: format!("{}/test-key-{}.pem", temp_dir.display(), test_id),
            captured_sni: CapturedSni::new(),
        })
    }

    /// Generate test certificates
    pub fn generate_certs(&self) -> Result<()> {
        // Install default crypto provider if not already set
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        use rcgen::{CertificateParams, DistinguishedName, DnType, SanType};
        use time::{Duration as TimeDuration, OffsetDateTime};

        let mut params = CertificateParams::default();
        params.not_before = OffsetDateTime::now_utc();
        params.not_after = OffsetDateTime::now_utc() + TimeDuration::days(1);
        params.subject_alt_names = vec![SanType::DnsName("*.vm.vers.sh".try_into()?)];

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "*.vm.vers.sh");
        params.distinguished_name = dn;

        let key_pair = rcgen::KeyPair::generate()?;
        let cert = params.self_signed(&key_pair)?;

        std::fs::write(&self.cert_path, cert.pem())?;
        std::fs::write(&self.key_path, key_pair.serialize_pem())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&self.key_path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&self.key_path, perms)?;
        }

        Ok(())
    }

    /// Clean up test certificates
    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.cert_path);
        let _ = std::fs::remove_file(&self.key_path);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Find an available TCP port for testing
pub fn find_available_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok(port)
}

/// Wait for a server to be ready on the given port
pub async fn wait_for_server(port: u16, timeout_secs: u64) -> Result<()> {
    let addr = format!("127.0.0.1:{}", port);
    let deadline = Duration::from_secs(timeout_secs);

    timeout(deadline, async {
        loop {
            match StdTcpStream::connect(&addr) {
                Ok(_) => return Ok::<_, anyhow::Error>(()),
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    })
    .await??;

    Ok(())
}

/// Create a TLS client connection with SNI
pub async fn connect_with_sni(
    port: u16,
    server_name: &str,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>> {
    // Install default crypto provider if not already set
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    use rustls::ClientConfig;
    use rustls::pki_types::ServerName;
    use std::sync::Arc;
    use tokio_rustls::TlsConnector;

    // Create client config that accepts self-signed certs for testing
    // For testing, we'll skip certificate verification
    let client_config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(client_config));
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

    let domain = ServerName::try_from(server_name.to_string())?;
    let tls_stream = connector.connect(domain, stream).await?;

    Ok(tls_stream)
}

/// Certificate verifier that accepts all certificates (for testing only!)
#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
