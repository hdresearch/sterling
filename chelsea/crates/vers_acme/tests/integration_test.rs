//! Integration tests for ACME client with Let's Encrypt staging.
//!
//! These tests are ignored by default because they require:
//! 1. A real domain name that you control
//! 2. An HTTP server running on port 80 to serve challenges
//! 3. The domain's DNS pointing to your server
//!
//! To run these tests:
//! 1. Edit the constants below with your email and domain
//! 2. Ensure you have an HTTP server that can serve the challenge files
//! 3. Run: cargo test --test integration_test -- --ignored --nocapture

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;
use vers_acme::{AcmeClient, AcmeConfig};

// ============================================================================
// CONFIGURATION - Set via environment variables or defaults
// ============================================================================

/// Your email address for ACME account registration
/// Set via ACME_TEST_EMAIL environment variable
static TEST_EMAIL: LazyLock<String> = LazyLock::new(|| {
    std::env::var("ACME_TEST_EMAIL")
        .expect("ACME_TEST_EMAIL environment variable must be set for integration tests")
});

/// Your domain name (must point to your server)
/// Set via ACME_TEST_DOMAIN environment variable
static TEST_DOMAIN: LazyLock<String> = LazyLock::new(|| {
    std::env::var("ACME_TEST_DOMAIN")
        .expect("ACME_TEST_DOMAIN environment variable must be set for integration tests")
});

/// Saved account credentials (JSON format)
/// Set via ACME_TEST_ACCOUNT_KEY environment variable
/// Leave empty to create a new account
static TEST_ACCOUNT_KEY: LazyLock<String> = LazyLock::new(|| {
    r#"{"id":"https://acme-staging-v02.api.letsencrypt.org/acme/acct/257342223","key_pkcs8":"MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQggmIgeGQ_WnO9lkRz_vHCx8Fj7fVyHuh5qYutphNIOeyhRANCAAQfM4mifnBaW2tXycyNQsDu8va1aWv3EMaE7wTZi4Wfq0SHP8rwG0wlp01ba2-nREFSlBBtGeWGbUv9w3IKyga0","directory":"https://acme-staging-v02.api.letsencrypt.org/directory"}"#.to_string()
});

/// HTTP server port for challenge serving
/// Set via ACME_TEST_HTTP_PORT environment variable
/// Port 80 is required for HTTP-01 validation (requires root/sudo)
/// Alternative: Use port 8080 and set up port forwarding (iptables, nginx, etc.)
static HTTP_PORT: LazyLock<u16> = LazyLock::new(|| {
    std::env::var("ACME_TEST_HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80)
});

// ============================================================================
// ACME Directory URLs
// ============================================================================

/// Let's Encrypt staging directory URL for testing.
///
/// These tests only use staging to prevent accidental production usage.
const LETSENCRYPT_STAGING_DIRECTORY: &str =
    "https://acme-staging-v02.api.letsencrypt.org/directory";

/// Helper function to create a staging ACME config.
fn staging_config(email: &str, account_key: &str) -> AcmeConfig {
    AcmeConfig {
        email: email.to_string(),
        directory_url: LETSENCRYPT_STAGING_DIRECTORY.to_string(),
        account_key: account_key.to_string(),
    }
}

#[tokio::test]
async fn test_load_existing_account() {
    // Initialize rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    if TEST_ACCOUNT_KEY.as_str().is_empty() {
        println!("Skipping test: TEST_ACCOUNT_KEY.as_str() is not set");
        println!("Run test_create_new_account first to get credentials");
        return;
    }

    let config = staging_config("vincent.thomas@hdr.is", TEST_ACCOUNT_KEY.as_str());
    let client = AcmeClient::new(config)
        .await
        .expect("Failed to load account");

    println!("Account loaded successfully!");

    // Verify we can get credentials back
    let credentials = client.account_credentials();
    assert!(!credentials.is_empty());
}

#[test]
fn test_config_validation() {
    // Initialize rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Unit test for configuration validation (doesn't need #[ignore])
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Test empty email
    rt.block_on(async {
        let config = AcmeConfig {
            email: "".to_string(),
            directory_url: LETSENCRYPT_STAGING_DIRECTORY.to_string(),
            account_key: "".to_string(),
        };

        let result = AcmeClient::new(config).await;
        assert!(result.is_err());
        if let Err(e) = result {
            println!("✓ Empty email rejected: {}", e);
        }
    });

    // Test empty directory URL
    rt.block_on(async {
        let config = AcmeConfig {
            email: "test@example.com".to_string(),
            directory_url: "".to_string(),
            account_key: TEST_ACCOUNT_KEY.to_string(),
        };

        let result = AcmeClient::new(config).await;
        assert!(result.is_err());
        if let Err(e) = result {
            println!("✓ Empty directory URL rejected: {}", e);
        }
    });
}

// ============================================================================
// Full End-to-End Test with HTTP Server
// ============================================================================

/// Simple HTTP challenge server
/// Serves ACME challenges from a shared HashMap
struct ChallengeServer {
    challenges: Arc<RwLock<HashMap<String, String>>>,
}

impl ChallengeServer {
    fn new(challenges: Arc<RwLock<HashMap<String, String>>>) -> Self {
        Self { challenges }
    }

    async fn handle_request(
        &self,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> Result<hyper::Response<http_body_util::Full<hyper::body::Bytes>>, std::convert::Infallible>
    {
        use http_body_util::Full;
        use hyper::body::Bytes;

        let path = req.uri().path();

        // Check if this is an ACME challenge request
        if path.starts_with("/.well-known/acme-challenge/") {
            let token = path.trim_start_matches("/.well-known/acme-challenge/");

            println!("  [HTTP] Request for challenge token: {}", token);

            let challenges = self.challenges.read().await;
            if let Some(key_auth) = challenges.get(token) {
                println!("  [HTTP] ✓ Serving challenge response");
                return Ok(hyper::Response::builder()
                    .status(200)
                    .header("Content-Type", "text/plain")
                    .body(Full::new(Bytes::from(key_auth.clone())))
                    .unwrap());
            } else {
                println!("  [HTTP] ✗ Challenge not found: {}", token);
                return Ok(hyper::Response::builder()
                    .status(404)
                    .body(Full::new(Bytes::from("Challenge not found")))
                    .unwrap());
            }
        }

        // Default response for other paths
        Ok(hyper::Response::builder()
            .status(200)
            .body(Full::new(Bytes::from("ACME Challenge Server")))
            .unwrap())
    }
}

/// Full end-to-end ACME certificate test with integrated HTTP server.
///
/// This test demonstrates the complete ACME workflow:
/// 1. Starts an HTTP server to serve challenges
/// 2. Creates an ACME account with Let's Encrypt staging
/// 3. Requests a certificate for TEST_DOMAIN.as_str()
/// 4. Automatically serves HTTP-01 challenges
/// 5. Notifies Let's Encrypt
/// 6. Waits for validation (Let's Encrypt will make HTTP requests)
/// 7. Retrieves and validates the certificate
///
/// Prerequisites:
/// - Set TEST_EMAIL.as_str() and TEST_DOMAIN.as_str() constants at the top of this file
/// - DNS for TEST_DOMAIN.as_str() must point to this server
/// - Run with root/sudo for port 80, or configure *HTTP_PORT and port forwarding
/// - Port must be accessible from the internet
///
/// Run with:
/// ```bash
/// sudo cargo test --test integration_test test_full_e2e_with_http_server -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "use './test-e2e.sh' to run this test"]
async fn test_full_e2e_with_http_server() {
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpListener;

    // Initialize rustls crypto provider (required for instant-acme)
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  End-to-End ACME Certificate Test with HTTP Server          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("Configuration:");
    println!("  Email:    {}", TEST_EMAIL.as_str());
    println!("  Domain:   {}", TEST_DOMAIN.as_str());
    println!("  Port:     {}", *HTTP_PORT);
    println!("  Directory: {}\n", LETSENCRYPT_STAGING_DIRECTORY);

    // Shared storage for challenges
    let challenges = Arc::new(RwLock::new(HashMap::new()));
    let challenges_clone = challenges.clone();

    // Start HTTP server in background
    println!("[1/7] Starting HTTP server on port {}...", *HTTP_PORT);

    let server_handle = tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", *HTTP_PORT);
        let listener = TcpListener::bind(&addr).await.expect(&format!(
            "Failed to bind to port {}. Make sure:\n  \
             1. You have root/sudo privileges (for port 80)\n  \
             2. No other service is using this port\n  \
             3. Or use port 8080 and set up port forwarding",
            *HTTP_PORT
        ));

        println!("  ✓ HTTP server listening on {}", addr);
        println!("  Ready to serve ACME challenges\n");

        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("  ✗ Accept error: {}", e);
                    continue;
                }
            };

            let io = TokioIo::new(stream);
            let challenges = challenges_clone.clone();

            tokio::spawn(async move {
                let server = ChallengeServer::new(challenges);

                let service = service_fn(move |req| {
                    let server = ChallengeServer::new(server.challenges.clone());
                    async move { server.handle_request(req).await }
                });

                if let Err(err) = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await
                {
                    eprintln!("  ✗ Server error: {}", err);
                }
            });
        }
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Step 2: Create ACME client
    println!("[2/7] Creating ACME client...");
    let config = staging_config(TEST_EMAIL.as_str(), TEST_ACCOUNT_KEY.as_str());
    let client = match AcmeClient::new(config).await {
        Ok(client) => {
            println!("  ✓ ACME client created\n");
            client
        }
        Err(e) => {
            eprintln!("  ✗ Failed to create client: {}", e);
            server_handle.abort();
            panic!("Client creation failed");
        }
    };

    // Save credentials
    let credentials = client.account_credentials();
    println!("  Account credentials (save for reuse):");
    println!("  {}\n", credentials);

    // Step 3: Request certificate
    println!(
        "[3/7] Requesting certificate for domain: {}",
        TEST_DOMAIN.as_str()
    );
    // Note: Only requesting TEST_DOMAIN, not wildcard domains
    // HTTP-01 challenges don't support wildcard domains (*.example.com)
    // Wildcard domains require DNS-01 challenges (use request_certificate_dns01)
    let domains = vec![TEST_DOMAIN.as_str().to_string()];
    let (mut order, received_challenges) = match client.request_certificate_http01(&domains).await {
        Ok(result) => {
            println!("  ✓ Certificate order created\n");
            result
        }
        Err(e) => {
            eprintln!("  ✗ Failed to create order: {}", e);
            server_handle.abort();
            panic!("Order creation failed");
        }
    };

    // Step 4: Store challenges in the server
    println!("[4/7] Configuring HTTP server with challenges...");
    {
        let mut challenges_map = challenges.write().await;
        for challenge in &received_challenges {
            println!("  Challenge for {}:", challenge.domain);
            println!("    Token: {}", challenge.token);
            println!(
                "    URL:   http://{}/.well-known/acme-challenge/{}",
                challenge.domain, challenge.token
            );
            println!("    Key Authorization: {}", challenge.key_authorization);
            challenges_map.insert(challenge.token.clone(), challenge.key_authorization.clone());
        }
    }
    println!(
        "  ✓ {} challenge(s) configured\n",
        received_challenges.len()
    );

    println!("  You can verify the server is working:");
    for challenge in &received_challenges {
        println!(
            "    curl http://{}/.well-known/acme-challenge/{}",
            challenge.domain, challenge.token
        );
    }
    println!();

    // Step 5: Notify ACME server
    println!("[5/7] Notifying ACME server that challenges are ready...");
    if let Err(e) = order.notify_ready().await {
        eprintln!("  ✗ Failed to notify ready: {}", e);
        server_handle.abort();
        panic!("Notify ready failed");
    }
    println!("  ✓ Challenges marked as ready\n");

    // Step 6: Wait for validation
    println!("[6/7] Waiting for ACME server to validate challenges...");
    println!("  This may take up to 5 minutes. The ACME server will make HTTP");
    println!(
        "  requests to http://{}/.well-known/acme-challenge/{{token}}",
        TEST_DOMAIN.as_str()
    );
    println!("  watching for validation requests...\n");

    match order.wait_for_validation().await {
        Ok(_) => {
            println!("  ✓ All challenges validated successfully!\n");
        }
        Err(e) => {
            eprintln!("  ✗ Challenge validation failed: {}", e);
            eprintln!("\n  Troubleshooting:");
            eprintln!(
                "  1. Verify DNS: dig {} (should point to this server)",
                TEST_DOMAIN.as_str()
            );
            eprintln!(
                "  2. Verify connectivity: curl http://{}/.well-known/acme-challenge/test",
                TEST_DOMAIN.as_str()
            );
            eprintln!(
                "  3. Check firewall: Port {} must be accessible from the internet",
                *HTTP_PORT
            );
            eprintln!("  4. Check server logs above for incoming requests");
            server_handle.abort();
            panic!("Validation failed");
        }
    }

    // Step 7: Finalize and get certificate
    println!("[7/7] Finalizing order and retrieving certificate...");
    let certificate = match order.finalize().await {
        Ok(cert) => {
            println!("  ✓ Certificate issued!\n");
            cert
        }
        Err(e) => {
            eprintln!("  ✗ Failed to finalize order: {}", e);
            server_handle.abort();
            panic!("Finalization failed");
        }
    };

    // Stop the server
    server_handle.abort();

    // Display certificate info
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  Certificate Successfully Obtained!                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("Certificate Details:");
    println!("  Domain:         {}", TEST_DOMAIN.as_str());
    println!(
        "  Expires at:     {} (Unix timestamp)",
        certificate.not_after
    );

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let days_until_expiry = (certificate.not_after - now) / 86400;
    println!("  Valid for:      {} days", days_until_expiry);
    println!(
        "  Cert length:    {} bytes",
        certificate.certificate_pem.len()
    );
    println!(
        "  Key length:     {} bytes",
        certificate.private_key_pem.len()
    );

    // Check renewal status
    println!("\nRenewal Status:");
    if certificate.needs_renewal(30) {
        println!("  ⚠️  Needs renewal (< 30 days until expiry)");
    } else {
        println!("  ✓ Valid (> 30 days until expiry)");
    }

    // Verify certificate format
    assert!(
        certificate
            .certificate_pem
            .contains("-----BEGIN CERTIFICATE-----")
    );
    assert!(
        certificate
            .certificate_pem
            .contains("-----END CERTIFICATE-----")
    );
    assert!(
        certificate
            .private_key_pem
            .contains("-----BEGIN PRIVATE KEY-----")
            || certificate
                .private_key_pem
                .contains("-----BEGIN EC PRIVATE KEY-----")
    );

    println!("\n⚠️  This is a STAGING certificate - not trusted by browsers");
    println!("   Use for testing only.\n");

    println!("✓ Test completed successfully!");
}
