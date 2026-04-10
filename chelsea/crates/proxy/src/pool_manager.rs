// =============================================================================
// TEMPORARY: Pool Manager Integration for vers.sh
// =============================================================================
//
// This module provides integration with the lockdown-shell pool manager for
// handling SSH connections to vers.sh. This is a temporary solution until
// we have a proper long-term architecture in place.
//
// TODO(temporary): Remove this module once we have a permanent solution
// for on-demand container/VM provisioning.
//
// =============================================================================

use serde::{Deserialize, Serialize};
use std::time::Duration;
use vers_config::VersConfig;

/// Special SNI hostname that triggers pool manager routing
/// TODO(temporary): This should be configurable
pub const POOL_MANAGER_SNI: &str = "shell.vers.sh";

/// Pool manager API endpoint
fn pool_manager_url() -> &'static str {
    &VersConfig::proxy().pool_manager_url
}

/// Pool manager auth token
fn pool_manager_auth_token() -> &'static str {
    &VersConfig::proxy().pool_manager_auth_token
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolContainer {
    pub id: String,
    pub name: String,
    pub status: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
struct AcquireResponse {
    container: PoolContainer,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

/// Check if the given SNI should be routed to the pool manager
/// TODO(temporary): This is a simple string match, should be more flexible
pub fn is_pool_manager_sni(sni: &str) -> bool {
    sni == POOL_MANAGER_SNI
}

/// Acquire a container from the pool manager
/// TODO(temporary): This uses a simple HTTP client, should be more robust
pub async fn acquire_container() -> anyhow::Result<PoolContainer> {
    let url = format!("{}/acquire", pool_manager_url());

    tracing::info!(url = %url, "[TEMP:pool_manager] Acquiring container from pool manager");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let auth_token = pool_manager_auth_token();
    let mut request = client.post(&url);

    if !auth_token.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", auth_token));
    }

    let response = request.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body: ErrorResponse = response.json().await.unwrap_or(ErrorResponse {
            error: "Unknown error".to_string(),
        });
        anyhow::bail!(
            "[TEMP:pool_manager] Failed to acquire container: {} - {}",
            status,
            body.error
        );
    }

    let acquire_response: AcquireResponse = response.json().await?;

    tracing::info!(
        container_id = %acquire_response.container.id,
        container_port = %acquire_response.container.port,
        "[TEMP:pool_manager] Acquired container"
    );

    Ok(acquire_response.container)
}

/// Release a container back to the pool manager (triggers replacement)
/// TODO(temporary): Fire-and-forget release, should handle errors better
pub async fn release_container(container_id: String) {
    let url = format!("{}/release/{}", pool_manager_url(), container_id);

    tracing::info!(
        container_id = %container_id,
        url = %url,
        "[TEMP:pool_manager] Releasing container"
    );

    // Fire and forget - we don't want to block on release
    let container_id = container_id.to_string();
    let auth_token = pool_manager_auth_token().to_string();
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build();

        match client {
            Ok(client) => {
                let mut request = client.post(&url);
                if !auth_token.is_empty() {
                    request = request.header("Authorization", format!("Bearer {}", auth_token));
                }

                if let Err(e) = request.send().await {
                    tracing::error!(
                        container_id = %container_id,
                        error = %e,
                        "[TEMP:pool_manager] Failed to release container"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "[TEMP:pool_manager] Failed to create HTTP client for release"
                );
            }
        }
    });
}

/// RAII guard that releases the container when dropped
/// TODO(temporary): This is a simple guard, should track metrics
pub struct PoolContainerGuard {
    container_id: String,
}

impl PoolContainerGuard {
    pub fn new(container_id: String) -> Self {
        Self { container_id }
    }
}

impl Drop for PoolContainerGuard {
    fn drop(&mut self) {
        // Release is fire-and-forget, spawns its own task
        tokio::spawn(release_container(self.container_id.clone()));
    }
}
