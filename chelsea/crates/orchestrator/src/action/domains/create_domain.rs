use chrono::{DateTime, Utc};
use dto_lib::domains::{READINESS_PROBE_PATH, READINESS_PROBE_RESPONSE};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, sync::OnceLock, time::Duration};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::action::{AuthzError, check_vm_access};
use crate::db::{ApiKeyEntity, DB, DBError, DomainInsertError, DomainsRepository};

/// Response type for domain operations.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DomainResponse {
    pub domain_id: Uuid,
    pub vm_id: Uuid,
    pub domain: String,
    pub created_at: DateTime<Utc>,
}

/// Create a custom domain for a VM.
#[derive(Debug, Clone)]
pub struct CreateDomain {
    vm_id: Uuid,
    domain: String,
    api_key: ApiKeyEntity,
    request_id: Option<String>,
}

impl CreateDomain {
    pub fn new(vm_id: Uuid, domain: String, api_key: ApiKeyEntity) -> Self {
        Self {
            vm_id,
            domain,
            api_key,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    /// Validate that the string is a valid domain name (RFC 1035 / RFC 1123).
    ///
    /// Rules:
    /// - Total length must be 1-253 characters
    /// - Must have at least two labels (e.g., "example.com")
    /// - Each label must be 1-63 characters
    /// - Labels can only contain ASCII alphanumeric characters and hyphens
    /// - Labels cannot start or end with a hyphen
    fn validate_domain_name(domain: &str) -> Result<(), CreateDomainError> {
        // Check total length
        if domain.is_empty() || domain.len() > 253 {
            return Err(CreateDomainError::InvalidDomain(
                "Domain must be 1-253 characters".to_string(),
            ));
        }

        // Split into labels
        let labels: Vec<&str> = domain.split('.').collect();

        // Must have at least two labels (domain + TLD)
        if labels.len() < 2 {
            return Err(CreateDomainError::InvalidDomain(
                "Domain must have at least two parts (e.g., example.com)".to_string(),
            ));
        }

        for label in &labels {
            // Check label length
            if label.is_empty() || label.len() > 63 {
                return Err(CreateDomainError::InvalidDomain(
                    "Each domain label must be 1-63 characters".to_string(),
                ));
            }

            // Check that label only contains alphanumeric characters and hyphens
            if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return Err(CreateDomainError::InvalidDomain(
                    "Domain labels can only contain alphanumeric characters and hyphens"
                        .to_string(),
                ));
            }

            // Labels cannot start or end with a hyphen
            if label.starts_with('-') || label.ends_with('-') {
                return Err(CreateDomainError::InvalidDomain(
                    "Domain labels cannot start or end with a hyphen".to_string(),
                ));
            }
        }

        // TLD (last label) must not be all numeric
        if let Some(tld) = labels.last() {
            if tld.chars().all(|c| c.is_ascii_digit()) {
                return Err(CreateDomainError::InvalidDomain(
                    "Top-level domain cannot be all numeric".to_string(),
                ));
            }
        }

        if domain.parse::<IpAddr>().is_ok() {
            return Err(CreateDomainError::InvalidDomain(
                "Domain cannot be an IP address literal".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CreateDomainError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("forbidden")]
    Forbidden,
    #[error("vm not found")]
    VmNotFound,
    #[error("domain already exists")]
    DomainAlreadyExists,
    #[error("invalid domain: {0}")]
    InvalidDomain(String),
    #[error("{0}")]
    DnsNotReady(String),
}

impl CreateDomain {
    pub async fn call(self, db: &DB) -> Result<DomainResponse, CreateDomainError> {
        // 1. Validate domain name
        Self::validate_domain_name(&self.domain)?;

        // 2. Check that the API key has access to the VM
        check_vm_access(db, &self.api_key, self.vm_id)
            .await
            .map_err(|e| match e {
                AuthzError::VmNotFound => CreateDomainError::VmNotFound,
                AuthzError::Forbidden => CreateDomainError::Forbidden,
                AuthzError::Db(db) => CreateDomainError::Db(db),
                AuthzError::CommitNotFound => CreateDomainError::Forbidden,
                AuthzError::TagNotFound => CreateDomainError::Forbidden,
            })?;

        self.ensure_domain_routed().await?;

        // 3. Insert the domain
        let entity = db
            .domains()
            .insert(self.api_key.id(), self.vm_id, &self.domain)
            .await
            .map_err(|e| match e {
                DomainInsertError::Db(db) => CreateDomainError::Db(db),
                DomainInsertError::DomainAlreadyExists => CreateDomainError::DomainAlreadyExists,
            })?;

        tracing::info!(
            domain_id = %entity.domain_id(),
            vm_id = %self.vm_id,
            domain = %self.domain,
            "Created domain"
        );

        Ok(DomainResponse {
            domain_id: entity.domain_id(),
            vm_id: entity.vm_id(),
            domain: entity.domain().to_string(),
            created_at: entity.created_at(),
        })
    }

    async fn ensure_domain_routed(&self) -> Result<(), CreateDomainError> {
        if is_reserved_testing_domain(&self.domain) {
            tracing::debug!(
                domain = %self.domain,
                "Skipping DNS readiness probe for reserved test domain"
            );
            return Ok(());
        }

        let url = format!("http://{}{}", self.domain, READINESS_PROBE_PATH);
        let client = domain_probe_client();

        // Trust that customer-controlled DNS points to their own infrastructure, but still
        // guard against obvious abuse (IP literals, oversized bodies) before accepting the domain.
        let mut response = client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|err| {
                CreateDomainError::DnsNotReady(format!(
                    "DNS for {} is not ready yet ({err})",
                    self.domain
                ))
            })?;

        validate_probe_status(&self.domain, response.status())?;

        let body = read_probe_body(&self.domain, &mut response).await?;

        validate_probe_body(&self.domain, body.trim())?;

        Ok(())
    }
}

fn domain_probe_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(Client::new)
}

fn validate_probe_status(domain: &str, status: StatusCode) -> Result<(), CreateDomainError> {
    if status == StatusCode::OK {
        Ok(())
    } else {
        Err(CreateDomainError::DnsNotReady(format!(
            "DNS for {} is not routed to Chelsea yet (HTTP {})",
            domain, status
        )))
    }
}

fn validate_probe_body(domain: &str, body: &str) -> Result<(), CreateDomainError> {
    if body == READINESS_PROBE_RESPONSE {
        Ok(())
    } else {
        Err(CreateDomainError::DnsNotReady(format!(
            "DNS for {} must point at the Chelsea proxy before creating the domain.",
            domain
        )))
    }
}

async fn read_probe_body(
    domain: &str,
    response: &mut reqwest::Response,
) -> Result<String, CreateDomainError> {
    const MAX_PROBE_BODY_BYTES: usize = READINESS_PROBE_RESPONSE.len() + 32;

    let mut body = Vec::new();

    while let Some(chunk) = response.chunk().await.map_err(|err| {
        CreateDomainError::DnsNotReady(format!("Unable to verify DNS for {} ({err})", domain))
    })? {
        if body.len() + chunk.len() > MAX_PROBE_BODY_BYTES {
            return Err(CreateDomainError::DnsNotReady(format!(
                "DNS probe for {} returned more than {MAX_PROBE_BODY_BYTES} bytes",
                domain
            )));
        }

        body.extend_from_slice(&chunk);
    }

    String::from_utf8(body).map_err(|err| {
        CreateDomainError::DnsNotReady(format!(
            "DNS probe for {} returned invalid UTF-8 ({err})",
            domain
        ))
    })
}

fn is_reserved_testing_domain(domain: &str) -> bool {
    const RESERVED_SECOND_LEVEL: [&str; 3] = ["example.com", "example.net", "example.org"];
    const RESERVED_TLDS: [&str; 4] = ["example", "test", "invalid", "localhost"];

    let domain = domain.to_ascii_lowercase();

    if RESERVED_SECOND_LEVEL
        .iter()
        .any(|suffix| matches_reserved_suffix(domain.as_str(), suffix))
    {
        return true;
    }

    if let Some(tld) = domain.rsplit('.').next() {
        if RESERVED_TLDS.iter().any(|reserved| reserved == &tld) {
            return true;
        }
    }

    false
}

fn matches_reserved_suffix(domain: &str, suffix: &str) -> bool {
    if domain == suffix {
        return true;
    }

    domain
        .strip_suffix(suffix)
        .map(|prefix| prefix.ends_with('.'))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_domains() {
        // Valid domains should pass
        assert!(CreateDomain::validate_domain_name("example.com").is_ok());
        assert!(CreateDomain::validate_domain_name("sub.example.com").is_ok());
        assert!(CreateDomain::validate_domain_name("my-domain.co.uk").is_ok());
        assert!(CreateDomain::validate_domain_name("test123.example.org").is_ok());
        assert!(CreateDomain::validate_domain_name("a.b").is_ok());
        assert!(CreateDomain::validate_domain_name("xn--n3h.com").is_ok()); // Punycode
        assert!(CreateDomain::validate_domain_name("123.example.com").is_ok()); // Numeric subdomain is ok
    }

    #[test]
    fn test_invalid_domain_empty() {
        let result = CreateDomain::validate_domain_name("");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_no_tld() {
        // Single label without TLD
        let result = CreateDomain::validate_domain_name("localhost");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_numeric_tld() {
        // TLD cannot be all numeric
        let result = CreateDomain::validate_domain_name("example.123");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_leading_hyphen() {
        let result = CreateDomain::validate_domain_name("-example.com");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_trailing_hyphen() {
        let result = CreateDomain::validate_domain_name("example-.com");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_special_chars() {
        let result = CreateDomain::validate_domain_name("exam_ple.com");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));

        let result = CreateDomain::validate_domain_name("exam@ple.com");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));

        let result = CreateDomain::validate_domain_name("exam ple.com");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_empty_label() {
        // Double dots create empty labels
        let result = CreateDomain::validate_domain_name("example..com");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));

        // Trailing dot creates empty label
        let result = CreateDomain::validate_domain_name("example.com.");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_too_long() {
        // Create a domain that exceeds 253 characters
        let long_label = "a".repeat(63);
        let long_domain = format!(
            "{}.{}.{}.{}.com",
            long_label, long_label, long_label, long_label
        );
        assert!(long_domain.len() > 253);
        let result = CreateDomain::validate_domain_name(&long_domain);
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_label_too_long() {
        // Single label exceeding 63 characters
        let long_label = "a".repeat(64);
        let domain = format!("{}.com", long_label);
        let result = CreateDomain::validate_domain_name(&domain);
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_email_format() {
        // Email addresses should be rejected
        let result = CreateDomain::validate_domain_name("user@example.com");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_numeric_only() {
        // Pure numbers without proper TLD
        let result = CreateDomain::validate_domain_name("123.456");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_invalid_domain_ipv4_literal() {
        let result = CreateDomain::validate_domain_name("192.168.0.1");
        assert!(matches!(result, Err(CreateDomainError::InvalidDomain(_))));
    }

    #[test]
    fn test_validate_probe_status_non_200() {
        let result = validate_probe_status("example.com", StatusCode::BAD_GATEWAY);
        assert!(matches!(result, Err(CreateDomainError::DnsNotReady(_))));
    }

    #[test]
    fn test_validate_probe_status_ok() {
        assert!(validate_probe_status("example.com", StatusCode::OK).is_ok());
    }

    #[test]
    fn test_validate_probe_body_mismatch() {
        let result = validate_probe_body("example.com", "something-else");
        assert!(matches!(result, Err(CreateDomainError::DnsNotReady(_))));
    }

    #[test]
    fn test_validate_probe_body_ok() {
        assert!(validate_probe_body("example.com", READINESS_PROBE_RESPONSE).is_ok());
    }

    #[test]
    fn test_reserved_testing_domain_logic() {
        assert!(is_reserved_testing_domain("foo.example.com"));
        assert!(is_reserved_testing_domain("foo.localhost"));
        assert!(!is_reserved_testing_domain("notexample.com"));
        assert!(!is_reserved_testing_domain("myapp.io"));
    }
}

impl_error_response!(CreateDomainError,
    CreateDomainError::Db(_) => INTERNAL_SERVER_ERROR,
    CreateDomainError::Forbidden => FORBIDDEN,
    CreateDomainError::VmNotFound => NOT_FOUND,
    CreateDomainError::DomainAlreadyExists => CONFLICT,
    CreateDomainError::InvalidDomain(_) => BAD_REQUEST,
    CreateDomainError::DnsNotReady(_) => UNPROCESSABLE_ENTITY,
);
