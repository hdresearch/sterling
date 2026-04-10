//! Error types for the LLM proxy.
//!
//! All errors implement RFC 7807 (Problem Details for HTTP APIs).
//! Responses use `content-type: application/problem+json`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rust_decimal::Decimal;
use serde::Serialize;

/// Base URI for problem types. Each error slug is appended to this.
/// These don't need to resolve to actual pages (RFC 7807 §3.1: "it is not
/// required to be dereferenceable"), but they SHOULD point to human-readable
/// documentation if we ever stand it up.
const PROBLEM_TYPE_BASE: &str = "https://api.vers.com/problems/";

/// RFC 7807 problem details JSON body.
#[derive(Debug, Serialize)]
pub struct ProblemDetails {
    /// URI reference identifying the problem type.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// Short, human-readable summary of the problem type.
    pub title: &'static str,
    /// HTTP status code.
    pub status: u16,
    /// Human-readable explanation specific to this occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// URI identifying the specific occurrence (e.g. request ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,

    // ─── Extension members ───────────────────────────────────────────
    /// Remaining credits (for budget errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<Decimal>,
    /// Current spend (for budget errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spend: Option<Decimal>,
    /// Total credits (for budget errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits: Option<Decimal>,
}

/// The canonical error type for the LLM proxy.
/// Each variant maps to a specific HTTP status and RFC 7807 problem type.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    // ─── Auth ────────────────────────────────────────────────────────
    #[error("missing API key")]
    MissingApiKey,

    #[error("invalid API key")]
    InvalidApiKey,

    #[error("API key has been revoked")]
    KeyRevoked,

    #[error("API key has expired")]
    KeyExpired,

    #[error("no credits available — please add credits to continue")]
    NoCredits,

    #[error("insufficient credits: ${remaining} remaining (${spend} spent of ${credits} credits)")]
    CreditsExhausted {
        remaining: Decimal,
        spend: Decimal,
        credits: Decimal,
    },

    #[error("API key budget exceeded")]
    BudgetExceeded,

    #[error("key does not have access to model '{model}'")]
    ModelNotAllowed { model: String },

    #[error("invalid admin API key")]
    InvalidAdminKey,

    #[error("admin API not enabled")]
    AdminNotEnabled,

    // ─── Request ─────────────────────────────────────────────────────
    #[error("missing '{field}' field")]
    MissingField { field: &'static str },

    #[error("model '{model}' not found")]
    ModelNotFound { model: String },

    #[error("invalid request body: {reason}")]
    BadRequest { reason: String },

    #[error("all providers failed for model '{model}': {detail}")]
    ProvidersFailed { model: String, detail: String },

    #[error("stream error")]
    StreamError,

    // ─── Not Found ───────────────────────────────────────────────────
    #[error("{entity} not found")]
    NotFound { entity: &'static str },

    // ─── Internal ────────────────────────────────────────────────────
    #[error("database error: {0}")]
    Database(#[from] DbError),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Re-export DbError from the billing crate.
pub use billing::error::DbError;

/// Provider-layer errors.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{provider} returned {status}: {body}")]
    Upstream {
        provider: &'static str,
        status: u16,
        body: String,
    },

    #[error("request to {provider} failed: {reason}")]
    Request {
        provider: &'static str,
        reason: String,
    },

    #[error("failed to parse response from {provider}: {reason}")]
    Parse {
        provider: &'static str,
        reason: String,
    },

    #[error("stream error from {provider}: {reason}")]
    Stream {
        provider: &'static str,
        reason: String,
    },
}

/// Config-layer errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file '{path}': {reason}")]
    ReadFile { path: String, reason: String },

    #[error("failed to parse config: {0}")]
    Parse(String),

    #[error("config validation error: {0}")]
    Validation(String),
}

// ─── Conversions ─────────────────────────────────────────────────────────────
// DbError conversions (From<bb8::RunError>, From<tokio_postgres::Error>) are
// defined in the billing crate where DbError lives.

impl From<ProviderError> for ProxyError {
    fn from(e: ProviderError) -> Self {
        ProxyError::ProvidersFailed {
            model: String::new(),
            detail: e.to_string(),
        }
    }
}

impl From<ConfigError> for ProxyError {
    fn from(e: ConfigError) -> Self {
        ProxyError::Internal(e.to_string())
    }
}

// ─── RFC 7807 response rendering ─────────────────────────────────────────────

impl ProxyError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::MissingApiKey | Self::InvalidApiKey | Self::KeyRevoked | Self::KeyExpired => {
                StatusCode::UNAUTHORIZED
            }
            Self::InvalidAdminKey => StatusCode::UNAUTHORIZED,
            Self::AdminNotEnabled => StatusCode::NOT_FOUND,
            Self::NoCredits | Self::CreditsExhausted { .. } | Self::BudgetExceeded => {
                StatusCode::TOO_MANY_REQUESTS
            }
            Self::ModelNotAllowed { .. } => StatusCode::FORBIDDEN,
            Self::MissingField { .. } | Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::ModelNotFound { .. } | Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::ProvidersFailed { .. } => StatusCode::BAD_GATEWAY,
            Self::StreamError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn problem_type(&self) -> &'static str {
        match self {
            Self::MissingApiKey => "missing-api-key",
            Self::InvalidApiKey => "invalid-api-key",
            Self::KeyRevoked => "key-revoked",
            Self::KeyExpired => "key-expired",
            Self::NoCredits => "no-credits",
            Self::CreditsExhausted { .. } => "credits-exhausted",
            Self::BudgetExceeded => "budget-exceeded",
            Self::ModelNotAllowed { .. } => "model-not-allowed",
            Self::InvalidAdminKey => "invalid-admin-key",
            Self::AdminNotEnabled => "admin-not-enabled",
            Self::MissingField { .. } => "missing-field",
            Self::ModelNotFound { .. } => "model-not-found",
            Self::BadRequest { .. } => "bad-request",
            Self::ProvidersFailed { .. } => "providers-failed",
            Self::StreamError => "stream-error",
            Self::NotFound { .. } => "not-found",
            Self::Database(_) => "database-error",
            Self::Internal(_) => "internal-error",
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::MissingApiKey => "Missing API Key",
            Self::InvalidApiKey => "Invalid API Key",
            Self::KeyRevoked => "API Key Revoked",
            Self::KeyExpired => "API Key Expired",
            Self::NoCredits => "No Credits",
            Self::CreditsExhausted { .. } => "Credits Exhausted",
            Self::BudgetExceeded => "Budget Exceeded",
            Self::ModelNotAllowed { .. } => "Model Not Allowed",
            Self::InvalidAdminKey => "Invalid Admin Key",
            Self::AdminNotEnabled => "Admin API Not Enabled",
            Self::MissingField { .. } => "Missing Required Field",
            Self::ModelNotFound { .. } => "Model Not Found",
            Self::BadRequest { .. } => "Bad Request",
            Self::ProvidersFailed { .. } => "Provider Failure",
            Self::StreamError => "Stream Error",
            Self::NotFound { .. } => "Not Found",
            Self::Database(_) => "Database Error",
            Self::Internal(_) => "Internal Server Error",
        }
    }

    fn to_problem_details(&self) -> ProblemDetails {
        let mut pd = ProblemDetails {
            problem_type: format!("{}{}", PROBLEM_TYPE_BASE, self.problem_type()),
            title: self.title(),
            status: self.status_code().as_u16(),
            detail: Some(self.to_string()),
            instance: None,
            remaining: None,
            spend: None,
            credits: None,
        };

        // Add extension members for budget-related errors
        if let Self::CreditsExhausted {
            remaining,
            spend,
            credits,
        } = self
        {
            pd.remaining = Some(*remaining);
            pd.spend = Some(*spend);
            pd.credits = Some(*credits);
        }

        pd
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        // Log internal errors — don't expose details to clients
        match &self {
            ProxyError::Database(e) => tracing::error!("database error: {e}"),
            ProxyError::Internal(e) => tracing::error!("internal error: {e}"),
            _ => {}
        }

        let status = self.status_code();
        let mut problem = self.to_problem_details();

        // Scrub internal details from 500s — don't leak implementation info
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            problem.detail = Some("An internal error occurred. Please try again later.".into());
        }

        let mut response = (status, axum::Json(problem)).into_response();
        // Static header value — infallible in practice, but avoid .expect() per CLAUDE.md.
        if let Ok(val) = "application/problem+json".parse() {
            response
                .headers_mut()
                .insert(axum::http::header::CONTENT_TYPE, val);
        }
        response
    }
}
