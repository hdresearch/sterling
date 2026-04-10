//! GitHub App authentication for generating repository clone URLs.
//!
//! Generates a JWT from the App ID + private key, exchanges it for an
//! installation access token, and builds an authenticated HTTPS clone URL.

use anyhow::{Context, Result, bail};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use vers_config::VersConfig;

/// JWT claims for GitHub App authentication.
#[derive(Serialize)]
struct GitHubAppClaims {
    /// Issued at (seconds since epoch)
    iat: i64,
    /// Expiration (seconds since epoch, max 10 minutes)
    exp: i64,
    /// GitHub App ID (issuer)
    iss: String,
}

/// Response from POST /app/installations/{id}/access_tokens
#[derive(Deserialize)]
struct InstallationToken {
    token: String,
}

/// Check if GitHub App deploy is configured.
pub fn is_configured() -> bool {
    let config = VersConfig::orchestrator();
    config.github_app_id.is_some() && config.github_app_private_key.is_some()
}

/// Generate a JWT for GitHub App authentication.
fn generate_app_jwt() -> Result<String> {
    let config = VersConfig::orchestrator();
    let app_id = config
        .github_app_id
        .as_ref()
        .context("GitHub App ID not configured")?;
    let private_key_pem = config
        .github_app_private_key
        .as_ref()
        .context("GitHub App private key not configured")?;

    // Replace literal \n with actual newlines (common in env vars)
    let pem = private_key_pem.replace("\\n", "\n");

    let now = chrono::Utc::now().timestamp();
    let claims = GitHubAppClaims {
        iat: now - 60,        // 60 seconds in the past to account for clock drift
        exp: now + (10 * 60), // 10 minutes max
        iss: app_id.clone(),
    };

    let header = Header::new(Algorithm::RS256);
    let key = EncodingKey::from_rsa_pem(pem.as_bytes())
        .context("Failed to parse GitHub App private key")?;

    jsonwebtoken::encode(&header, &claims, &key).context("Failed to encode GitHub App JWT")
}

/// Get an installation access token from GitHub.
async fn get_installation_token(installation_id: i64) -> Result<String> {
    let jwt = generate_app_jwt()?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "https://api.github.com/app/installations/{installation_id}/access_tokens"
        ))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "vers-orchestrator")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("Failed to request installation token from GitHub")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("GitHub returned {status} when requesting installation token: {body}");
    }

    let token: InstallationToken = resp
        .json()
        .await
        .context("Failed to parse installation token response")?;

    Ok(token.token)
}

/// Build an authenticated HTTPS clone URL for a GitHub repository.
///
/// `full_name` should be in `owner/repo` format.
pub async fn get_clone_url(installation_id: i64, full_name: &str) -> Result<String> {
    let token = get_installation_token(installation_id).await?;
    let (owner, repo) = full_name
        .split_once('/')
        .context("Invalid repo full_name format")?;
    Ok(format!(
        "https://x-access-token:{token}@github.com/{owner}/{repo}.git"
    ))
}
