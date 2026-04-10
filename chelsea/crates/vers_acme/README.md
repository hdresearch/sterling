# vers_acme

A high-level Rust ACME (Automated Certificate Management Environment) client library for obtaining TLS certificates using HTTP-01 or DNS-01 validation.

Built on top of [`instant-acme`](https://crates.io/crates/instant-acme), this library provides a simplified API focused solely on the ACME protocol, leaving I/O operations (file storage, HTTP serving, DNS management) to the caller.

## Features

- **HTTP-01 validation** - For regular domains (requires HTTP server on port 80)
- **DNS-01 validation** - For all domains including wildcards (requires DNS TXT record management)
- **High-level API** - Simple builder-style interface
- **Certificate renewal** - Built-in expiry checking and renewal logic
- **Account persistence** - Serialize/deserialize account credentials
- **No I/O beyond ACME** - Caller controls all file, network, and DNS operations
- **Let's Encrypt ready** - Works with staging and production environments

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
vers_acme = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

## Usage Examples

### HTTP-01 Challenge (Regular Domains)

```rust
use vers_acme::{AcmeClient, AcmeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure client for Let's Encrypt staging
    let config = AcmeConfig {
        email: "admin@example.com".to_string(),
        directory_url: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
        account_key: None, // Creates new account
    };

    // Create ACME client
    let client = AcmeClient::new(config).await?;

    // Save account credentials for reuse
    let credentials = client.account_credentials();
    std::fs::write("account.json", credentials)?;

    // Request certificate with HTTP-01 validation
    let domains = vec!["example.com".to_string()];
    let (mut order, challenges) = client
        .request_certificate_http01(&domains)
        .await?;

    // Serve challenges via HTTP server (your responsibility)
    for challenge in &challenges {
        println!("Serve '{}' at http://{}/.well-known/acme-challenge/{}",
                 challenge.key_authorization,
                 challenge.domain,
                 challenge.token);

        // TODO: Set up your HTTP server to serve the challenges
        // Example: Write to file, update nginx config, etc.
    }

    // Notify ACME server that challenges are ready
    order.notify_ready().await?;

    // Wait for validation (polls every 5 seconds)
    order.wait_for_validation().await?;

    // Get the certificate
    let certificate = order.finalize().await?;

    // Save certificate and key
    std::fs::write("cert.pem", certificate.certificate_pem)?;
    std::fs::write("key.pem", certificate.private_key_pem)?;

    // Check if renewal is needed
    if certificate.needs_renewal(30) {
        println!("Certificate expires in < 30 days, should renew!");
    }

    Ok(())
}
```

### DNS-01 Challenge (Including Wildcards)

```rust
use vers_acme::{AcmeClient, AcmeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AcmeConfig {
        email: "admin@example.com".to_string(),
        directory_url: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
        account_key: None,
    };

    let client = AcmeClient::new(config).await?;

    // Request certificate for wildcard domain (requires DNS-01)
    let domains = vec!["*.example.com".to_string()];
    let (mut order, challenges) = client
        .request_certificate_dns01(&domains)
        .await?;

    // Create DNS TXT records (your responsibility)
    for challenge in &challenges {
        println!("Create DNS TXT record:");
        println!("  Name:  {}", challenge.record_name);
        println!("  Value: {}", challenge.record_value);

        // TODO: Create the DNS TXT record via your DNS provider's API
        // Example: Route53, Cloudflare, DigitalOcean, etc.
    }

    // Wait for DNS propagation before continuing
    println!("Waiting for DNS propagation...");
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

    // Notify ACME server that challenges are ready
    order.notify_ready().await?;

    // Wait for validation
    order.wait_for_validation().await?;

    // Get the certificate
    let certificate = order.finalize().await?;

    // Save certificate
    std::fs::write("wildcard-cert.pem", certificate.certificate_pem)?;
    std::fs::write("wildcard-key.pem", certificate.private_key_pem)?;

    Ok(())
}
```

## Using Existing Account

```rust
let account_key = std::fs::read_to_string("account.json")?;
let config = AcmeConfig {
    email: "admin@example.com".to_string(),
    directory_url: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
    account_key: Some(account_key),
};

let client = AcmeClient::new(config).await?;
```

## Multiple Domains (SAN)

```rust
let domains = vec![
    "example.com".to_string(),
    "www.example.com".to_string(),
    "api.example.com".to_string(),
];

let (order, challenges) = client.request_certificate(&domains).await?;
// You'll receive one challenge per domain
```

## Certificate Renewal

```rust
// Load existing certificate
let cert_pem = std::fs::read_to_string("cert.pem")?;
let certificate = Certificate::from_pem(&cert_pem)?;

// Check if renewal needed (30 days before expiry)
if certificate.needs_renewal(30) {
    println!("Time to renew!");
    // Request new certificate using the same process
}
```

## Let's Encrypt Directory

### Staging (For Testing)
```rust
const STAGING: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";
```
- Use for **all testing and development**
- Certificates are **NOT trusted** by browsers
- Higher rate limits
- Safe to experiment with
- No cost

### For Production Use

To use production Let's Encrypt:
```rust
const PRODUCTION: &str = "https://acme-v02.api.letsencrypt.org/directory";

let config = AcmeConfig {
    email: "your-email@example.com".to_string(),
    directory_url: PRODUCTION.to_string(),
    account_key: None,
};
```

**Production considerations:**
- Rate limit: 50 certificates per domain per week
- Implement proper error handling and retry logic
- Set up monitoring and alerting
- Automate certificate renewal (30 days before expiry)
- Test thoroughly with staging first

⚠️ **Always test with staging before using production!**

## Running the Example

The example uses Let's Encrypt **staging only** for safe testing.

```bash
# Run the interactive example
cargo run --example obtain_certificate -- \
    --email your-email@example.com \
    --domain your-domain.example.com

# Multiple domains
cargo run --example obtain_certificate -- \
    --email your-email@example.com \
    --domain example.com \
    --domain www.example.com

# Use saved account
cargo run --example obtain_certificate -- \
    --email your-email@example.com \
    --domain example.com \
    --account-key account_credentials.json
```

**Note:** The example only supports staging to prevent accidental production use. For production, integrate the library into your application with proper error handling and monitoring.

## Running Integration Tests

Integration tests require a real domain and HTTP server:

```bash
# Run all tests except integration tests
cargo test

# Run integration tests (requires setup)
cargo test --test integration_test -- --ignored --nocapture
```

### Manual Integration Tests

Before running manual integration tests:
1. Edit `tests/integration_test.rs` and set the constants:
   - `TEST_EMAIL` - Your email address
   - `TEST_DOMAIN` - Your domain name (must point to your server)
   - `HTTP_PORT` - Port 80 (requires root) or 8080 with port forwarding
2. Ensure DNS points to your server
3. Set up port forwarding if using port 8080

### Full End-to-End Test with HTTP Server

The `test_full_e2e_with_http_server` test provides a complete automated workflow:

```bash
# Run the full E2E test (requires root for port 80 or port forwarding)
sudo cargo test --test integration_test test_full_e2e_with_http_server -- --ignored --nocapture
```

**What this test does:**
1. Starts an HTTP server on port 80 (configurable via `HTTP_PORT` constant)
2. Creates an ACME account
3. Requests a certificate for your domain
4. Automatically serves HTTP-01 challenges
5. Notifies Let's Encrypt
6. Waits for validation (Let's Encrypt makes HTTP requests to verify)
7. Retrieves and validates the certificate

**Requirements:**
- Root access (for port 80) or configured port forwarding
- Domain DNS pointing to your server
- Port 80 accessible from the internet
- Firewall configured to allow incoming connections

**Test output includes:**
- Challenge URLs to test with curl
- Troubleshooting steps
- Real-time HTTP request logging
- Certificate details upon success

## How HTTP-01 Validation Works

1. **Request Certificate**: Client asks ACME server for certificate
2. **Receive Challenges**: Server responds with challenges for each domain
3. **Serve Challenge**: You serve challenge at `http://{domain}/.well-known/acme-challenge/{token}`
4. **Notify Server**: Client tells server challenges are ready
5. **Server Validates**: ACME server makes HTTP requests to verify
6. **Get Certificate**: If valid, server issues certificate

## Architecture

This library handles **only ACME protocol operations**:
- ✅ Creating/loading ACME accounts
- ✅ Requesting certificates
- ✅ Managing challenge lifecycle
- ✅ Retrieving certificates
- ✅ Parsing certificate expiry

You handle **all I/O operations**:
- ❌ Serving HTTP challenges (use your web server)
- ❌ Storing account credentials (use files, database, etc.)
- ❌ Storing certificates (use files, secrets manager, etc.)
- ❌ Configuring your application (use env vars, config files, etc.)

This separation of concerns makes the library flexible and suitable for various deployment scenarios.

## API Reference

### `AcmeClient`

- `new(config: AcmeConfig) -> Result<Self>` - Create client (creates or loads account)
- `account_credentials() -> String` - Get account credentials for storage
- `request_certificate_http01(&self, domains: &[String]) -> Result<(CertificateOrder, Vec<Http01Challenge>)>` - Request certificate with HTTP-01 validation
- `request_certificate_dns01(&self, domains: &[String]) -> Result<(CertificateOrder, Vec<Dns01Challenge>)>` - Request certificate with DNS-01 validation (supports wildcards)

### `CertificateOrder`

- `notify_ready(&mut self) -> Result<()>` - Notify challenges are ready
- `wait_for_validation(&mut self) -> Result<()>` - Wait for ACME validation
- `finalize(self) -> Result<Certificate>` - Get the certificate

### `Certificate`

- `new(cert_pem: String, key_pem: String) -> Result<Self>` - Create from PEM
- `from_pem(cert_pem: &str) -> Result<Self>` - Parse existing certificate
- `needs_renewal(&self, days_before: u64) -> bool` - Check if renewal needed

### `Http01Challenge`

- `token: String` - Token for URL path
- `key_authorization: String` - Content to serve
- `domain: String` - Domain being validated

### `Dns01Challenge`

- `domain: String` - Domain being validated
- `record_name: String` - Full DNS record name (e.g., "_acme-challenge.example.com")
- `record_value: String` - TXT record value (base64url-encoded SHA256 digest)

## Error Handling

All operations return `Result<T, AcmeError>` with these error types:

- `Protocol` - ACME protocol error
- `InvalidCertificate` - Certificate parsing/validation error
- `ChallengeFailed` - Challenge validation failed
- `InvalidConfig` - Invalid configuration
- `AccountKey` - Account key error
- `OrderError` - Order processing error
- `CertificateParsing` - Certificate parsing error

## Dependencies

- `instant-acme` - ACME protocol implementation
- `tokio` - Async runtime
- `rcgen` - CSR generation
- `x509-parser` - Certificate parsing
- `thiserror` - Error handling
- `serde`/`serde_json` - Serialization

## License

This project is part of the Chelsea workspace. See the workspace root for license information.

## Contributing

This crate is part of a larger project. Please refer to the main repository for contribution guidelines.

## Resources

- [ACME Protocol (RFC 8555)](https://tools.ietf.org/html/rfc8555)
- [Let's Encrypt Documentation](https://letsencrypt.org/docs/)
- [instant-acme Documentation](https://docs.rs/instant-acme/)

## FAQ

### Why doesn't the library serve HTTP challenges automatically?

Different deployments have different requirements:
- Some need nginx/apache integration
- Some need cloud load balancer integration
- Some need custom HTTP servers
- Some need to update existing servers

By returning challenge data, you can integrate with any HTTP infrastructure.

### Can I use this with DNS-01 validation?

Yes! DNS-01 is fully supported via the `request_certificate_dns01()` method. DNS-01 is required for wildcard certificates and works for all domain types.

### How do I automate certificate renewal?

1. Periodically check `certificate.needs_renewal(30)`
2. If true, request new certificate using the same workflow
3. Replace old certificate with new one
4. Reload your application/server

### What about rate limits?

Let's Encrypt production has these limits:
- 50 certificates per domain per week
- 5 duplicate certificates per week
- 300 new accounts per IP per 3 hours

**Always test with staging first** to avoid hitting production limits!

### Can I use this in production?

Yes! The library is production-ready. Make sure to:
1. Test thoroughly with staging first
2. Handle errors appropriately
3. Implement proper certificate renewal
4. Monitor certificate expiry
5. Have alerting for failures
