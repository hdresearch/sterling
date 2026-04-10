//! ACME client implementation.

use crate::{
    AcmeConfig, AcmeError, Result,
    order::CertificateOrder,
    types::{Dns01Challenge, Http01Challenge},
};
use instant_acme::{Account, AccountCredentials, ChallengeType, Identifier, NewOrder};
use std::sync::Arc;

/// Inner state of the ACME client.
struct AcmeClientInner {
    account: Account,
    credentials: AccountCredentials,
}

/// High-level ACME client for certificate management.
///
/// Wraps the `instant-acme` Account and provides a simplified API
/// focused on ACME protocol operations.
#[derive(Clone)]
pub struct AcmeClient {
    inner: Arc<AcmeClientInner>,
}

impl AcmeClient {
    /// Create a new ACME client.
    ///
    /// If `config.account_key` is provided, loads an existing account.
    /// Otherwise, creates a new account with the ACME provider.
    ///
    /// # Arguments
    ///
    /// * `config` - ACME configuration including email and directory URL
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory URL is invalid
    /// - Account creation or loading fails
    /// - The account key format is invalid
    ///
    /// # Example
    ///
    /// ```no_run
    /// use vers_acme::{AcmeClient, AcmeConfig};
    ///
    /// # async fn example() -> Result<(), vers_acme::AcmeError> {
    /// let config = AcmeConfig {
    ///     email: "admin@example.com".to_string(),
    ///     directory_url: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
    ///     account_key: None,
    /// };
    ///
    /// let client = AcmeClient::new(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(config: AcmeConfig) -> Result<Self> {
        if config.email.is_empty() {
            return Err(AcmeError::InvalidConfig(
                "Email cannot be empty".to_string(),
            ));
        }

        if config.directory_url.is_empty() {
            return Err(AcmeError::InvalidConfig(
                "Directory URL cannot be empty".to_string(),
            ));
        }

        let (account, credentials) = Self::load_account(&config.account_key).await?;

        Ok(Self {
            inner: Arc::new(AcmeClientInner {
                account,
                credentials,
            }),
        })
    }

    /// Get the account credentials for storage.
    ///
    /// Returns a JSON string containing the account credentials that can be
    /// stored by the caller and later provided as `account_key` in `AcmeConfig`
    /// to restore the account.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use vers_acme::{AcmeClient, AcmeConfig};
    ///
    /// # async fn example() -> Result<(), vers_acme::AcmeError> {
    /// let config = AcmeConfig {
    ///     email: "admin@example.com".to_string(),
    ///     directory_url: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
    ///     account_key: None,
    /// };
    ///
    /// let client = AcmeClient::new(config).await?;
    /// let credentials = client.account_credentials();
    /// // Store credentials for later use
    /// # Ok(())
    /// # }
    /// ```
    pub fn account_credentials(&self) -> String {
        serde_json::to_string(&self.inner.credentials)
            .expect("Failed to serialize account credentials")
    }

    /// Request a certificate for the specified domains using HTTP-01 validation.
    ///
    /// Returns a `CertificateOrder` and a list of HTTP-01 challenges that must
    /// be fulfilled by the caller by serving content via HTTP on port 80.
    ///
    /// **Note:** HTTP-01 does NOT support wildcard domains. Use `request_certificate_dns01`
    /// for wildcard certificates.
    ///
    /// # Arguments
    ///
    /// * `domains` - List of domain names to include in the certificate
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The domain list is empty
    /// - Creating the order fails
    /// - No HTTP-01 challenges are available for any domain
    ///
    /// # Example
    ///
    /// ```no_run
    /// use vers_acme::{AcmeClient, AcmeConfig};
    ///
    /// # async fn example(client: AcmeClient) -> Result<(), vers_acme::AcmeError> {
    /// let domains = vec!["example.com".to_string()];
    /// let (mut order, challenges) = client.request_certificate_http01(&domains).await?;
    ///
    /// for challenge in &challenges {
    ///     println!("Serve {} at http://{}/.well-known/acme-challenge/{}",
    ///              challenge.key_authorization, challenge.domain, challenge.token);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip(self), fields(domains = ?domains))]
    pub async fn request_certificate_http01(
        &self,
        domains: &[String],
    ) -> Result<(CertificateOrder, Vec<Http01Challenge>)> {
        tracing::info!("Starting HTTP-01 certificate request");

        if domains.is_empty() {
            tracing::error!("Domain list is empty");
            return Err(AcmeError::InvalidConfig(
                "Domain list cannot be empty".to_string(),
            ));
        }

        // Create identifiers for each domain
        let identifiers: Vec<Identifier> = domains
            .iter()
            .map(|domain| Identifier::Dns(domain.clone()))
            .collect();

        tracing::debug!("Creating new order with identifiers");

        // Create a new order
        let mut order = self
            .inner
            .account
            .new_order(&NewOrder::new(&identifiers))
            .await
            .map_err(|e| {
                tracing::error!(error = ?e, "Failed to create new order");
                e
            })?;

        tracing::info!("Order created successfully");

        // Collect HTTP-01 challenges
        let mut challenges = Vec::new();
        let mut challenge_urls = Vec::new();

        tracing::info!("Fetching authorizations");
        let mut authorizations = order.authorizations();
        let mut auth_count = 0usize;

        while let Some(authz_result) = authorizations.next().await {
            let mut authz = authz_result.map_err(|e| {
                tracing::error!(error = ?e, "Failed to fetch authorization");
                e
            })?;
            let domain = authz.identifier().to_string();

            tracing::debug!(
                auth_idx = auth_count,
                domain = %domain,
                status = ?authz.status,
                "Processing authorization"
            );

            // Obtain the HTTP-01 challenge handle
            let challenge_handle = authz.challenge(ChallengeType::Http01).ok_or_else(|| {
                tracing::error!(domain = %domain, "No HTTP-01 challenge available");
                AcmeError::ChallengeFailed(format!(
                    "No HTTP-01 challenge available for domain: {}",
                    domain
                ))
            })?;

            let token = challenge_handle.token.clone();
            let key_authorization = challenge_handle.key_authorization().as_str().to_string();

            tracing::info!(
                domain = %domain,
                token = %token,
                key_auth_len = key_authorization.len(),
                "Generated HTTP-01 challenge"
            );

            // Store the challenge URL for later notification
            challenge_urls.push(challenge_handle.url.clone());

            challenges.push(Http01Challenge {
                token,
                key_authorization,
                domain: domain.clone(),
            });
            auth_count += 1;
        }

        tracing::info!(auth_count, "Retrieved authorizations");

        tracing::info!(
            challenge_count = challenges.len(),
            "HTTP-01 certificate request complete"
        );

        Ok((
            CertificateOrder::new(order, domains.to_vec(), challenge_urls),
            challenges,
        ))
    }

    /// Request a certificate for the specified domains using DNS-01 validation.
    ///
    /// Returns a `CertificateOrder` and a list of DNS-01 challenges that must
    /// be fulfilled by the caller by creating DNS TXT records.
    ///
    /// **Note:** DNS-01 supports all domains including wildcards (*.example.com).
    ///
    /// # Arguments
    ///
    /// * `domains` - List of domain names to include in the certificate
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The domain list is empty
    /// - Creating the order fails
    /// - No DNS-01 challenges are available for any domain
    ///
    /// # Example
    ///
    /// ```no_run
    /// use vers_acme::{AcmeClient, AcmeConfig};
    ///
    /// # async fn example(client: AcmeClient) -> Result<(), vers_acme::AcmeError> {
    /// let domains = vec!["*.example.com".to_string()];
    /// let (mut order, challenges) = client.request_certificate_dns01(&domains).await?;
    ///
    /// for challenge in &challenges {
    ///     println!("Create DNS TXT record:");
    ///     println!("  Name:  {}", challenge.record_name);
    ///     println!("  Value: {}", challenge.record_value);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn request_certificate_dns01(
        &self,
        domains: &[String],
    ) -> Result<(CertificateOrder, Vec<Dns01Challenge>)> {
        if domains.is_empty() {
            return Err(AcmeError::InvalidConfig(
                "Domain list cannot be empty".to_string(),
            ));
        }

        // Create identifiers for each domain
        let identifiers: Vec<Identifier> = domains
            .iter()
            .map(|domain| Identifier::Dns(domain.clone()))
            .collect();

        // Create a new order
        let mut order = self
            .inner
            .account
            .new_order(&NewOrder::new(&identifiers))
            .await?;

        // Collect DNS-01 challenges
        let mut challenges = Vec::new();
        let mut challenge_urls = Vec::new();
        let mut authorizations = order.authorizations();

        while let Some(authz_result) = authorizations.next().await {
            let mut authz = authz_result?;

            // Extract domain from the identifier
            let domain = authz.identifier().to_string();

            // Find the DNS-01 challenge handle
            let challenge_handle = authz.challenge(ChallengeType::Dns01).ok_or_else(|| {
                AcmeError::ChallengeFailed(format!(
                    "No DNS-01 challenge available for domain: {}",
                    domain
                ))
            })?;

            let record_value = challenge_handle.key_authorization().dns_value();

            // Construct the full DNS record name
            let record_name = format!("_acme-challenge.{}", domain);

            // Store the challenge URL for later notification
            challenge_urls.push(challenge_handle.url.clone());

            challenges.push(Dns01Challenge {
                domain: domain.clone(),
                record_name,
                record_value,
            });
        }

        Ok((
            CertificateOrder::new(order, domains.to_vec(), challenge_urls),
            challenges,
        ))
    }

    /// Load an existing ACME account.
    async fn load_account(account_key: &str) -> Result<(Account, AccountCredentials)> {
        let credentials: AccountCredentials = serde_json::from_str(account_key)
            .map_err(|e| AcmeError::AccountKey(format!("Failed to parse account key: {}", e)))?;

        // Parse it again to get a second copy (AccountCredentials doesn't implement Clone)
        let credentials_copy: AccountCredentials = serde_json::from_str(account_key)
            .map_err(|e| AcmeError::AccountKey(format!("Failed to parse account key: {}", e)))?;

        let builder = Account::builder()
            .map_err(|e| AcmeError::AccountKey(format!("Failed to build account client: {}", e)))?;

        let account = builder.from_credentials(credentials).await.map_err(|e| {
            AcmeError::AccountKey(format!("Failed to load account from credentials: {}", e))
        })?;

        Ok((account, credentials_copy))
    }
}
