//! ACME client implementation for automated certificate management.
//!
//! This crate provides a high-level API for obtaining and managing TLS certificates
//! using the ACME protocol (RFC 8555) with HTTP-01 validation. It wraps the `instant-acme`
//! library and focuses solely on ACME protocol operations, leaving I/O operations
//! (file storage, HTTP serving) to the caller.
//!
//! # Example
//!
//! ```no_run
//! use vers_acme::{AcmeClient, AcmeConfig};
//!
//! # async fn example() -> Result<(), vers_acme::AcmeError> {
//! // Create client
//! let config = AcmeConfig {
//!     email: "admin@example.com".to_string(),
//!     directory_url: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
//!     account_key: None,
//! };
//! let client = AcmeClient::new(config).await?;
//!
//! // Request certificate with HTTP-01 validation
//! let domains = vec!["example.com".to_string()];
//! let (mut order, challenges) = client.request_certificate_http01(&domains).await?;
//!
//! // Serve challenges (caller's responsibility)
//! for challenge in &challenges {
//!     // Serve challenge.key_authorization at /.well-known/acme-challenge/{challenge.token}
//! }
//!
//! // Complete the order
//! order.notify_ready().await?;
//! order.wait_for_validation().await?;
//! let cert = order.finalize().await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod order;
mod types;

pub use client::AcmeClient;
pub use order::CertificateOrder;
pub use types::{AcmeConfig, Certificate, Dns01Challenge, Http01Challenge};

/// Errors that can occur during ACME operations.
#[derive(Debug, thiserror::Error)]
pub enum AcmeError {
    /// Error from the ACME protocol implementation
    #[error("ACME protocol error: {0}")]
    Protocol(#[from] instant_acme::Error),

    /// Invalid certificate format or data
    #[error("Invalid certificate: {0}")]
    InvalidCertificate(String),

    /// Challenge validation failed
    #[error("Challenge failed: {0}")]
    ChallengeFailed(String),

    /// Invalid configuration provided
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Account key error
    #[error("Account key error: {0}")]
    AccountKey(String),

    /// Order processing error
    #[error("Order error: {0}")]
    OrderError(String),

    /// Certificate parsing error
    #[error("Certificate parsing error: {0}")]
    CertificateParsing(String),
}

/// Result type for ACME operations.
pub type Result<T> = std::result::Result<T, AcmeError>;
