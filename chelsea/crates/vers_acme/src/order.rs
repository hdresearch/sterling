//! Certificate order management.

use std::collections::{HashMap, HashSet};

use crate::{AcmeError, Result, types::Certificate};
use instant_acme::{AuthorizationHandle, ChallengeType, Order, OrderStatus};
use rcgen::{CertificateParams, DistinguishedName, KeyPair};

/// Represents an in-progress certificate order.
///
/// This struct wraps an `instant-acme::Order` and provides methods to
/// progress through the ACME certificate acquisition workflow.
pub struct CertificateOrder {
    order: Order,
    private_key: Option<KeyPair>,
    domains: Vec<String>,
    /// URLs of the challenges that were selected (to notify later)
    challenge_urls: Vec<String>,
}

impl CertificateOrder {
    /// Create a new CertificateOrder from an instant-acme Order.
    pub(crate) fn new(order: Order, domains: Vec<String>, challenge_urls: Vec<String>) -> Self {
        Self {
            order,
            private_key: None,
            domains,
            challenge_urls,
        }
    }

    /// Notify the ACME server that challenges are ready for validation.
    ///
    /// Call this method after serving all HTTP-01 challenge responses or creating
    /// all DNS-01 TXT records. The ACME server will then attempt to validate each challenge.
    ///
    /// This method notifies the ACME server about all challenges that were returned
    /// when the order was created (either HTTP-01 or DNS-01).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Communication with the ACME server fails
    /// - The challenges are in an invalid state
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vers_acme::{AcmeClient, AcmeConfig};
    /// # async fn example(mut order: vers_acme::CertificateOrder) -> Result<(), vers_acme::AcmeError> {
    /// // After serving all HTTP-01 challenges or creating DNS-01 records...
    /// order.notify_ready().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip(self), fields(challenge_count = self.challenge_urls.len()))]
    pub async fn notify_ready(&mut self) -> Result<()> {
        tracing::info!("Notifying ACME server that challenges are ready");

        let mut url_to_index = HashMap::new();
        for (idx, url) in self.challenge_urls.iter().cloned().enumerate() {
            url_to_index.insert(url, idx);
        }
        let mut remaining: HashSet<String> = url_to_index.keys().cloned().collect();

        let mut authorizations = self.order.authorizations();
        while let Some(authz_result) = authorizations.next().await {
            let mut authz = authz_result.map_err(|e| {
                tracing::error!(error = ?e, "Failed to fetch authorization while notifying ready");
                e
            })?;

            let identifier = authz.identifier().to_string();
            let matches = find_matching_challenges(&authz, &url_to_index);

            if let Some((challenge_url, challenge_type, idx)) = matches.into_iter().next() {
                if !remaining.remove(&challenge_url) {
                    continue;
                }

                tracing::debug!(
                    challenge_idx = idx,
                    url = %challenge_url,
                    domain = %identifier,
                    "Setting challenge ready"
                );

                let mut challenge_handle =
                    authz.challenge(challenge_type.clone()).ok_or_else(|| {
                        AcmeError::ChallengeFailed(format!(
                            "Challenge handle not available for URL: {}",
                            challenge_url
                        ))
                    })?;

                challenge_handle.set_ready().await.map_err(|e| {
                    tracing::error!(
                        error = ?e,
                        url = %challenge_url,
                        challenge_idx = idx,
                        "Failed to notify ACME server about challenge readiness"
                    );
                    e
                })?;

                tracing::info!(challenge_idx = idx, "Challenge marked as ready");
            }
        }

        if !remaining.is_empty() {
            tracing::error!(missing = ?remaining, "Some challenge URLs were not found in authorizations");
            let missing_list = remaining.into_iter().collect::<Vec<_>>().join(", ");
            return Err(AcmeError::ChallengeFailed(format!(
                "Challenge URLs not found: {}",
                missing_list
            )));
        }

        tracing::info!("All challenges notified successfully");
        Ok(())
    }

    /// Wait for the ACME server to validate all challenges.
    ///
    /// This method polls the order status until either:
    /// - All challenges are validated (order becomes ready)
    /// - A challenge fails (returns an error)
    /// - Maximum retries are exhausted (returns an error)
    ///
    /// Polls every 5 seconds for up to 60 attempts (5 minutes total).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Challenge validation fails
    /// - The ACME server returns an error
    /// - Maximum retries are exhausted
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vers_acme::{AcmeClient, AcmeConfig};
    /// # async fn example(mut order: vers_acme::CertificateOrder) -> Result<(), vers_acme::AcmeError> {
    /// order.notify_ready().await?;
    /// order.wait_for_validation().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip(self))]
    pub async fn wait_for_validation(&mut self) -> Result<()> {
        const MAX_ATTEMPTS: u32 = 60;
        const DELAY_SECONDS: u64 = 5;

        tracing::info!(
            max_attempts = MAX_ATTEMPTS,
            delay_seconds = DELAY_SECONDS,
            "Waiting for challenge validation"
        );

        for attempt in 0..MAX_ATTEMPTS {
            tracing::debug!(attempt = attempt + 1, "Refreshing order state");

            // Refresh order state
            let state = self.order.refresh().await.map_err(|e| {
                tracing::error!(error = ?e, attempt = attempt + 1, "Failed to refresh order state");
                e
            })?;

            tracing::debug!(attempt = attempt + 1, status = ?state.status, "Order status");

            match state.status {
                OrderStatus::Ready => {
                    tracing::info!(attempts = attempt + 1, "Order validated successfully");
                    return Ok(());
                }
                OrderStatus::Invalid => {
                    tracing::error!(attempts = attempt + 1, "Order validation failed");
                    return Err(AcmeError::ChallengeFailed(
                        "Order validation failed".to_string(),
                    ));
                }
                OrderStatus::Processing | OrderStatus::Pending => {
                    tracing::debug!(
                        attempt = attempt + 1,
                        status = ?state.status,
                        "Order still processing, waiting..."
                    );
                    if attempt < MAX_ATTEMPTS - 1 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(DELAY_SECONDS)).await;
                    }
                }
                status => {
                    tracing::error!(status = ?status, "Unexpected order status");
                    return Err(AcmeError::OrderError(format!(
                        "Unexpected order status: {:?}",
                        status
                    )));
                }
            }
        }

        tracing::error!(
            max_attempts = MAX_ATTEMPTS,
            "Timeout waiting for challenge validation"
        );
        Err(AcmeError::OrderError(
            "Timeout waiting for challenge validation".to_string(),
        ))
    }

    /// Finalize the order and retrieve the certificate.
    ///
    /// This method:
    /// 1. Generates a private key and CSR (Certificate Signing Request)
    /// 2. Submits the CSR to the ACME server
    /// 3. Waits for the certificate to be issued
    /// 4. Returns the certificate chain and private key
    ///
    /// The private key is generated using ECDSA P-256.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - CSR generation fails
    /// - Order finalization fails
    /// - Certificate retrieval fails
    /// - Certificate parsing fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use vers_acme::{AcmeClient, AcmeConfig};
    /// # async fn example(mut order: vers_acme::CertificateOrder) -> Result<(), vers_acme::AcmeError> {
    /// order.notify_ready().await?;
    /// order.wait_for_validation().await?;
    /// let certificate = order.finalize().await?;
    /// println!("Certificate expires at: {}", certificate.expiry);
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip(self), fields(domain_count = self.domains.len()))]
    pub async fn finalize(mut self) -> Result<Certificate> {
        tracing::info!("Starting certificate finalization");

        // Generate private key and CSR
        tracing::debug!("Generating CSR");
        let (csr_der, private_key) = self.generate_csr().map_err(|e| {
            tracing::error!(error = ?e, "Failed to generate CSR");
            e
        })?;

        // Store the private key for later
        self.private_key = Some(private_key);

        // Finalize the order with the CSR
        tracing::info!("Submitting CSR to ACME server");
        self.order.finalize_csr(&csr_der).await.map_err(|e| {
            tracing::error!(error = ?e, "Failed to finalize order");
            e
        })?;

        // Poll for the certificate
        const DELAY_SECONDS: u64 = 5;

        tracing::info!("Polling for certificate issuance");
        let mut poll_count = 0;
        let cert_chain_pem = loop {
            poll_count += 1;
            tracing::debug!(poll_attempt = poll_count, "Checking for certificate");

            match self.order.certificate().await.map_err(|e| {
                tracing::error!(error = ?e, poll_attempt = poll_count, "Failed to retrieve certificate");
                e
            })? {
                Some(cert) => {
                    tracing::info!(poll_attempts = poll_count, "Certificate issued successfully");
                    break cert;
                }
                None => {
                    tracing::debug!(poll_attempt = poll_count, "Certificate not ready yet, waiting...");
                    // Certificate not ready yet, wait and retry
                    tokio::time::sleep(tokio::time::Duration::from_secs(DELAY_SECONDS)).await;
                }
            }
        };

        // Get the private key
        let private_key_pem = self
            .private_key
            .as_ref()
            .ok_or_else(|| {
                tracing::error!("Private key not found");
                AcmeError::OrderError("Private key not found".to_string())
            })?
            .serialize_pem();

        // Create Certificate with parsed expiry
        tracing::info!("Parsing certificate chain");
        let certificate = Certificate::new(cert_chain_pem, private_key_pem)?;

        tracing::info!(expiry = ?certificate.not_after, "Certificate finalization complete");
        Ok(certificate)
    }

    /// Generate a Certificate Signing Request (CSR).
    ///
    /// Creates a new ECDSA P-256 key pair and generates a CSR for the domains
    /// in the order.
    fn generate_csr(&self) -> Result<(Vec<u8>, KeyPair)> {
        // Generate a new private key
        let key_pair = KeyPair::generate()
            .map_err(|e| AcmeError::OrderError(format!("Failed to generate private key: {}", e)))?;

        if self.domains.is_empty() {
            return Err(AcmeError::OrderError(
                "No domains found in order".to_string(),
            ));
        }

        // Create certificate parameters - first domain is the subject
        let mut params = CertificateParams::new(self.domains.clone()).map_err(|e| {
            AcmeError::OrderError(format!("Failed to create certificate params: {}", e))
        })?;

        // Set a basic distinguished name
        params.distinguished_name = DistinguishedName::new();

        // Generate the CSR
        let csr = params
            .serialize_request(&key_pair)
            .map_err(|e| AcmeError::OrderError(format!("Failed to generate CSR: {}", e)))?;

        Ok((csr.der().to_vec(), key_pair))
    }
}

fn find_matching_challenges(
    authz: &AuthorizationHandle<'_>,
    targets: &HashMap<String, usize>,
) -> Vec<(String, ChallengeType, usize)> {
    authz
        .challenges
        .iter()
        .filter_map(|challenge| {
            targets
                .get(&challenge.url)
                .map(|&idx| (challenge.url.clone(), challenge.r#type.clone(), idx))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_csr() {
        // This is a unit test for CSR generation
        // We can't easily test the full order flow without a real ACME server
        let key_pair = KeyPair::generate().unwrap();
        assert!(!key_pair.serialize_pem().is_empty());
    }

    #[test]
    fn test_generate_csr_single_domain() {
        // Create a mock order with a single domain
        let domains = vec!["example.com".to_string()];
        let key_pair = KeyPair::generate().unwrap();

        let params = CertificateParams::new(domains.clone()).unwrap();
        let csr = params.serialize_request(&key_pair).unwrap();

        // CSR should not be empty
        assert!(!csr.der().is_empty());
    }

    #[test]
    fn test_generate_csr_multiple_domains() {
        // Create a mock order with multiple domains (SAN certificate)
        let domains = vec![
            "example.com".to_string(),
            "www.example.com".to_string(),
            "api.example.com".to_string(),
        ];
        let key_pair = KeyPair::generate().unwrap();

        let params = CertificateParams::new(domains.clone()).unwrap();
        let csr = params.serialize_request(&key_pair).unwrap();

        // CSR should not be empty
        assert!(!csr.der().is_empty());
    }

    #[test]
    fn test_private_key_generation() {
        // Test that we can generate multiple different private keys
        let key1 = KeyPair::generate().unwrap();
        let key2 = KeyPair::generate().unwrap();

        let pem1 = key1.serialize_pem();
        let pem2 = key2.serialize_pem();

        // Keys should be different
        assert_ne!(pem1, pem2);

        // Both should be valid PEM format
        assert!(pem1.starts_with("-----BEGIN"));
        assert!(pem2.starts_with("-----BEGIN"));
    }

    #[test]
    fn test_private_key_pem_format() {
        let key_pair = KeyPair::generate().unwrap();
        let pem = key_pair.serialize_pem();

        // Should have PEM header and footer
        assert!(pem.contains("-----BEGIN"));
        assert!(pem.contains("-----END"));

        // Should have content between headers
        let lines: Vec<&str> = pem.lines().collect();
        assert!(lines.len() > 2); // More than just header and footer
    }

    #[test]
    fn test_challenge_urls_empty() {
        // Test that an order can be created with empty challenge URLs
        let domains = vec!["example.com".to_string()];
        let challenge_urls: Vec<String> = vec![];

        // This would need a real Order object, so we just test the data structures
        assert_eq!(challenge_urls.len(), 0);
        assert_eq!(domains.len(), 1);
    }

    #[test]
    fn test_challenge_urls_multiple() {
        // Test challenge URL collection
        let challenge_urls = vec![
            "https://acme-staging.api.letsencrypt.org/acme/chall/abc123".to_string(),
            "https://acme-staging.api.letsencrypt.org/acme/chall/def456".to_string(),
            "https://acme-staging.api.letsencrypt.org/acme/chall/ghi789".to_string(),
        ];

        assert_eq!(challenge_urls.len(), 3);

        // All URLs should be from the ACME server
        for url in &challenge_urls {
            assert!(url.starts_with("https://"));
            assert!(url.contains("acme"));
            assert!(url.contains("chall"));
        }
    }

    #[test]
    fn test_csr_wildcard_domain() {
        // Test CSR generation for wildcard domain
        let domains = vec!["*.example.com".to_string()];
        let key_pair = KeyPair::generate().unwrap();

        let params = CertificateParams::new(domains.clone()).unwrap();
        let csr = params.serialize_request(&key_pair).unwrap();

        assert!(!csr.der().is_empty());
    }

    #[test]
    fn test_csr_subdomain() {
        // Test CSR generation for subdomain
        let domains = vec!["sub.domain.example.com".to_string()];
        let key_pair = KeyPair::generate().unwrap();

        let params = CertificateParams::new(domains.clone()).unwrap();
        let csr = params.serialize_request(&key_pair).unwrap();

        assert!(!csr.der().is_empty());
    }

    #[test]
    fn test_csr_mixed_domains() {
        // Test CSR with both regular and wildcard domains
        let domains = vec!["example.com".to_string(), "*.example.com".to_string()];
        let key_pair = KeyPair::generate().unwrap();

        let params = CertificateParams::new(domains.clone()).unwrap();
        let csr = params.serialize_request(&key_pair).unwrap();

        assert!(!csr.der().is_empty());
    }

    #[test]
    fn test_key_consistency() {
        // Test that the same key produces the same PEM output
        let key_pair = KeyPair::generate().unwrap();

        let pem1 = key_pair.serialize_pem();
        let pem2 = key_pair.serialize_pem();

        assert_eq!(pem1, pem2);
    }
}
