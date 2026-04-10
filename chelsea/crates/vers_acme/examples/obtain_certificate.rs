//! Example: Obtain a certificate using ACME HTTP-01 validation.
//!
//! This example demonstrates the complete workflow for obtaining a TLS certificate
//! from Let's Encrypt staging using the vers_acme client.
//!
//! ## Prerequisites
//!
//! 1. A domain name that you control
//! 2. The domain's DNS must point to a server you can run this on
//! 3. Port 80 must be accessible for HTTP-01 challenges
//! 4. You need to set up an HTTP server to serve the challenges
//!
//! ## Usage
//!
//! ```bash
//! cargo run --example obtain_certificate -- \
//!     --email your-email@example.com \
//!     --domain your-domain.example.com
//! ```
//!
//! ## IMPORTANT: Testing with Staging Only
//!
//! This example uses Let's Encrypt **staging** environment only.
//! Staging certificates are NOT trusted by browsers but are perfect for testing.
//!
//! For production use, integrate this library into your application with proper
//! error handling, monitoring, and renewal automation.

use std::path::PathBuf;
use vers_acme::{AcmeClient, AcmeConfig};

const LETSENCRYPT_STAGING: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";

#[derive(Debug)]
struct Args {
    email: String,
    domains: Vec<String>,
    account_key_file: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut email = None;
        let mut domains = Vec::new();
        let mut account_key_file = None;

        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;

        while i < args.len() {
            match args[i].as_str() {
                "--email" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--email requires a value".to_string());
                    }
                    email = Some(args[i].clone());
                }
                "--domain" | "-d" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--domain requires a value".to_string());
                    }
                    domains.push(args[i].clone());
                }
                "--account-key" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--account-key requires a value".to_string());
                    }
                    account_key_file = Some(PathBuf::from(&args[i]));
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                arg => {
                    return Err(format!("Unknown argument: {}", arg));
                }
            }
            i += 1;
        }

        let email = email.ok_or("--email is required")?;
        if domains.is_empty() {
            return Err("At least one --domain is required".to_string());
        }
        let account_key_file = account_key_file.ok_or("--account-key is required")?;

        Ok(Args {
            email,
            domains,
            account_key_file,
        })
    }
}

fn print_help() {
    println!(
        r#"
Obtain Certificate - ACME HTTP-01 Example (STAGING ONLY)

USAGE:
    obtain_certificate [OPTIONS] --email <EMAIL> --domain <DOMAIN>

OPTIONS:
    --email <EMAIL>              Email address for ACME account
    --domain <DOMAIN>            Domain(s) to include in certificate (can be specified multiple times)
    -d <DOMAIN>                  Short form of --domain
    --account-key <FILE>         Path to saved account credentials (JSON)
    -h, --help                   Print this help message

EXAMPLES:
    # Single domain
    cargo run --example obtain_certificate -- \
        --email admin@example.com \
        --domain example.com

    # Multiple domains
    cargo run --example obtain_certificate -- \
        --email admin@example.com \
        --domain example.com \
        --domain www.example.com

    # Using saved account credentials
    cargo run --example obtain_certificate -- \
        --email admin@example.com \
        --domain example.com \
        --account-key account.json

NOTES:
    - This example uses Let's Encrypt STAGING only
    - Staging certificates are NOT trusted by browsers
    - You must serve HTTP-01 challenges on port 80
    - See the example output for challenge details
    - For production use, integrate the library into your application
"#
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse arguments
    let args = Args::parse().unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        eprintln!("\nUse --help for usage information");
        std::process::exit(1);
    });

    println!("=== ACME Certificate Request ===\n");
    println!("Email: {}", args.email);
    println!("Domains: {}", args.domains.join(", "));
    println!("Directory: Let's Encrypt Staging");

    println!("\n⚠️  Using STAGING environment (certificates won't be trusted)");
    println!("   This is for testing only. For production, integrate the library into your app.");

    // Load account key if provided
    let path = &args.account_key_file;
    println!("\nLoading account key from: {}", path.display());
    let account_key = std::fs::read_to_string(path)?;

    // Step 1: Create ACME client
    println!("\n[1/6] Creating ACME client...");
    let config = AcmeConfig {
        email: args.email.clone(),
        directory_url: LETSENCRYPT_STAGING.to_string(),
        account_key,
    };

    let client = AcmeClient::new(config).await?;
    println!("✓ Client created successfully");

    // Step 2: Request certificate
    println!("\n[2/6] Requesting certificate...");
    let (mut order, challenges) = client.request_certificate_http01(&args.domains).await?;
    println!("✓ Order created successfully");

    // Step 3: Display challenges
    println!("\n[3/6] HTTP-01 Challenges:");
    println!("\n⚠️  YOU MUST SERVE THESE CHALLENGES AT THE URLS BELOW:\n");

    for (i, challenge) in challenges.iter().enumerate() {
        println!("Challenge #{}: {}", i + 1, challenge.domain);
        println!("─────────────────────────────────────────");
        println!(
            "URL:  http://{}/.well-known/acme-challenge/{}",
            challenge.domain, challenge.token
        );
        println!("File: {}", challenge.token);
        println!("Content:\n{}\n", challenge.key_authorization);
    }

    println!("Example using a temporary HTTP server:");
    println!("─────────────────────────────────────────");
    println!("mkdir -p .well-known/acme-challenge");
    for challenge in &challenges {
        println!(
            "echo '{}' > .well-known/acme-challenge/{}",
            challenge.key_authorization, challenge.token
        );
    }
    println!("python3 -m http.server 80  # Or use nginx, apache, etc.\n");

    // Step 4: Wait for user confirmation
    println!("[4/6] Press Enter once you have set up the HTTP server and challenges...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    // Step 5: Notify and validate
    println!("\n[5/6] Notifying ACME server and waiting for validation...");
    order.notify_ready().await?;
    println!("✓ Challenges marked as ready");

    println!("\nWaiting for ACME server to validate (may take up to 5 minutes)...");
    order.wait_for_validation().await?;
    println!("✓ All challenges validated successfully!");

    // Step 6: Finalize and get certificate
    println!("\n[6/6] Finalizing order and retrieving certificate...");
    let certificate = order.finalize().await?;
    println!("✓ Certificate issued!");

    // Save certificate and private key
    let cert_file = format!("{}.crt", args.domains[0]);
    let key_file = format!("{}.key", args.domains[0]);

    std::fs::write(&cert_file, &certificate.certificate_pem)?;
    std::fs::write(&key_file, &certificate.private_key_pem)?;

    println!("\n=== Success! ===");
    println!("Certificate saved to: {}", cert_file);
    println!("Private key saved to: {}", key_file);
    println!(
        "Certificate expires at: {} (Unix timestamp)",
        certificate.not_after
    );

    // Calculate days until expiry
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let days_until_expiry = (certificate.not_after - now) / 86400;
    println!("Days until expiry: {}", days_until_expiry);

    // Renewal check
    if certificate.needs_renewal(30) {
        println!("\n⚠️  Certificate needs renewal (< 30 days until expiry)");
    } else {
        println!("\n✓ Certificate is valid (> 30 days until expiry)");
    }

    println!("\n⚠️  STAGING CERTIFICATE - Not trusted by browsers!");
    println!("   This is for testing only.");

    Ok(())
}
