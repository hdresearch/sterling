//! Core data types for ACME client operations.

use crate::{AcmeError, Result};
use serde::{Deserialize, Serialize};
use x509_parser::prelude::*;

/// Configuration for ACME client.
///
/// All configuration values are provided by the caller - no environment
/// variables or file I/O is performed.
#[derive(Debug, Clone)]
pub struct AcmeConfig {
    /// Contact email for the ACME account
    pub email: String,

    /// ACME directory URL (e.g., Let's Encrypt staging or production)
    pub directory_url: String,

    /// existing account key in PEM format.
    pub account_key: String,
}

/// HTTP-01 challenge data to be served by the caller.
///
/// The caller must serve `key_authorization` at the URL:
/// `http://{domain}/.well-known/acme-challenge/{token}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Http01Challenge {
    /// Challenge token to use in the URL path
    pub token: String,

    /// Key authorization string to serve as the HTTP response body
    pub key_authorization: String,

    /// Domain being validated
    pub domain: String,
}

/// DNS-01 challenge data to be created by the caller.
///
/// The caller must create a DNS TXT record:
/// - **Name:** `_acme-challenge.{domain}` (or just `_acme-challenge` for apex domains)
/// - **Type:** TXT
/// - **Value:** `{record_value}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dns01Challenge {
    /// Domain being validated
    pub domain: String,

    /// Full DNS record name (e.g., "_acme-challenge.example.com")
    pub record_name: String,

    /// TXT record value (base64url-encoded SHA256 digest)
    pub record_value: String,
}

/// Certificate with metadata.
///
/// Contains the certificate chain and private key in PEM format,
/// along with parsed expiry information for renewal checking.
#[derive(Debug, Clone)]
pub struct Certificate {
    /// Full certificate chain in PEM format
    pub certificate_pem: String,

    /// Private key in PEM format
    pub private_key_pem: String,

    /// Certificate not-after time (Unix timestamp)
    pub not_after: i64,

    /// Certificate not-before time (Unix timestamp)
    pub not_before: i64,
}

impl Certificate {
    /// Create a Certificate from PEM-encoded certificate and private key.
    ///
    /// Parses the certificate to extract the expiry date.
    pub fn new(certificate_pem: String, private_key_pem: String) -> Result<Self> {
        let (not_before, not_after) = Self::parse_validity(&certificate_pem)?;

        Ok(Self {
            certificate_pem,
            private_key_pem,
            not_after,
            not_before,
        })
    }

    /// Parse an existing certificate PEM to create a Certificate instance.
    ///
    /// Note: This does not include a private key, so `private_key_pem` will be empty.
    /// Useful for checking renewal status of existing certificates.
    pub fn from_pem(certificate_pem: &str) -> Result<Self> {
        let (not_before, not_after) = Self::parse_validity(certificate_pem)?;

        Ok(Self {
            certificate_pem: certificate_pem.to_string(),
            private_key_pem: String::new(),
            not_after,
            not_before,
        })
    }

    /// Check if the certificate needs renewal.
    ///
    /// Returns true if the certificate expires within the specified number of days.
    ///
    /// # Arguments
    ///
    /// * `days_before` - Number of days before expiry to consider renewal needed
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vers_acme::Certificate;
    /// # fn example(cert: Certificate) {
    /// // Renew if less than 30 days until expiry
    /// if cert.needs_renewal(30) {
    ///     // Initiate renewal process
    /// }
    /// # }
    /// ```
    pub fn needs_renewal(&self, days_before: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time before Unix epoch")
            .as_secs() as i64;

        let days_in_seconds = (days_before as i64) * 86400;
        let renewal_threshold = now + days_in_seconds;

        self.not_after <= renewal_threshold
    }

    /// Parse the validity period from a PEM-encoded certificate.
    ///
    /// Returns (not_before, not_after) as Unix timestamps.
    fn parse_validity(certificate_pem: &str) -> Result<(i64, i64)> {
        // Find the first PEM block that looks like a certificate
        let lines: Vec<&str> = certificate_pem.lines().collect();
        let mut in_cert = false;
        let mut der_base64 = String::new();

        for line in lines {
            if line.starts_with("-----BEGIN CERTIFICATE-----") {
                in_cert = true;
                continue;
            }
            if line.starts_with("-----END CERTIFICATE-----") {
                break;
            }
            if in_cert {
                der_base64.push_str(line.trim());
            }
        }

        if der_base64.is_empty() {
            return Err(AcmeError::CertificateParsing(
                "No certificate found in PEM".to_string(),
            ));
        }

        // Decode base64
        use base64::{Engine, engine::general_purpose};
        let der = general_purpose::STANDARD.decode(&der_base64).map_err(|e| {
            AcmeError::CertificateParsing(format!("Failed to decode base64: {}", e))
        })?;

        // Parse the X.509 certificate
        let (_, cert) = X509Certificate::from_der(&der).map_err(|e| {
            AcmeError::CertificateParsing(format!("Failed to parse X.509 certificate: {}", e))
        })?;

        // Get the validity period
        let validity = cert.validity();
        let not_before = validity.not_before.timestamp();
        let not_after = validity.not_after.timestamp();

        Ok((not_before, not_after))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http01_challenge_serialization() {
        let challenge = Http01Challenge {
            token: "test_token".to_string(),
            key_authorization: "test_key_auth".to_string(),
            domain: "example.com".to_string(),
        };

        // Test that it can be serialized (serde derives should work)
        let json = serde_json::to_string(&challenge).expect("Failed to serialize");
        assert!(json.contains("test_token"));
        assert!(json.contains("test_key_auth"));
        assert!(json.contains("example.com"));

        // Test deserialization
        let deserialized: Http01Challenge =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized.token, challenge.token);
        assert_eq!(deserialized.key_authorization, challenge.key_authorization);
        assert_eq!(deserialized.domain, challenge.domain);
    }

    #[test]
    fn test_dns01_challenge_serialization() {
        let challenge = Dns01Challenge {
            domain: "example.com".to_string(),
            record_name: "_acme-challenge.example.com".to_string(),
            record_value: "test_value".to_string(),
        };

        // Test that it can be serialized
        let json = serde_json::to_string(&challenge).expect("Failed to serialize");
        assert!(json.contains("example.com"));
        assert!(json.contains("_acme-challenge.example.com"));
        assert!(json.contains("test_value"));

        // Test deserialization
        let deserialized: Dns01Challenge =
            serde_json::from_str(&json).expect("Failed to deserialize");
        assert_eq!(deserialized.domain, challenge.domain);
        assert_eq!(deserialized.record_name, challenge.record_name);
        assert_eq!(deserialized.record_value, challenge.record_value);
    }

    #[test]
    fn test_certificate_needs_renewal_threshold() {
        // Create a certificate that expires in 20 days
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let not_after_20_days = now + (20 * 86400);

        let cert = Certificate {
            certificate_pem: "fake_cert".to_string(),
            private_key_pem: "fake_key".to_string(),
            not_after: not_after_20_days,
            not_before: now - (10 * 86400),
        };

        // Should need renewal if threshold is 30 days
        assert!(cert.needs_renewal(30));

        // Should NOT need renewal if threshold is 15 days
        assert!(!cert.needs_renewal(15));

        // Should need renewal if threshold is exactly 20 days (not_after <= threshold)
        assert!(cert.needs_renewal(20));

        // Should need renewal if threshold is 21 days
        assert!(cert.needs_renewal(21));

        // Should NOT need renewal if threshold is 19 days
        assert!(!cert.needs_renewal(19));
    }

    #[test]
    fn test_certificate_already_expired() {
        // Create a certificate that expired 10 days ago
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let not_after_past = now - (10 * 86400);

        let cert = Certificate {
            certificate_pem: "fake_cert".to_string(),
            private_key_pem: "fake_key".to_string(),
            not_after: not_after_past,
            not_before: now - (100 * 86400),
        };

        // Should need renewal regardless of threshold
        assert!(cert.needs_renewal(0));
        assert!(cert.needs_renewal(30));
        assert!(cert.needs_renewal(90));
    }

    #[test]
    fn test_certificate_far_future_expiry() {
        // Create a certificate that expires in 365 days
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let not_after_365_days = now + (365 * 86400);

        let cert = Certificate {
            certificate_pem: "fake_cert".to_string(),
            private_key_pem: "fake_key".to_string(),
            not_after: not_after_365_days,
            not_before: now - (10 * 86400),
        };

        // Should NOT need renewal with standard thresholds
        assert!(!cert.needs_renewal(30));
        assert!(!cert.needs_renewal(60));
        assert!(!cert.needs_renewal(90));

        // Should need renewal only with very large threshold
        assert!(cert.needs_renewal(366));
    }
}
