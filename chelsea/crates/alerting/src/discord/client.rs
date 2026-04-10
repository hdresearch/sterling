use crate::processor::AlertProcessor;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// A Discord webhook client
pub struct Client {
    http_client: reqwest::Client,
    webhook_url: String,
}

impl Client {
    /// Construct a new Discord webhook client, reusing an existing HTTP client.
    pub fn with_client(webhook_url: impl AsRef<str>, http_client: reqwest::Client) -> Self {
        Self {
            http_client,
            webhook_url: webhook_url.as_ref().to_string(),
        }
    }

    /// Execute the webhook with a simple message
    pub async fn send_message(&self, message: impl AsRef<str>) {
        let payload = WebhookPayload::with_content(message.as_ref().to_string());

        match self
            .http_client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                warn!(
                    status = %response.status(),
                    "discord webhook responded with non-success status"
                );
            }
            Err(err) => {
                warn!(%err, "failed to send discord webhook alert");
            }
        }
    }
}

#[async_trait]
impl AlertProcessor for Client {
    async fn billing_subscription_lookup_failed(
        &self,
        org_id: &uuid::Uuid,
        customer_id: &str,
        detail: &str,
    ) {
        let message = format!(
            "Billing subscription lookup failed for org {org_id} (customer {customer_id}): {detail}",
        );
        self.send_message(message).await;
    }

    async fn chelsea_resource_threshold_exceeded(
        &self,
        node_id: &uuid::Uuid,
        resource_name: &str,
        threshold: f32,
        current: f32,
    ) {
        let message = format!(
            "Resource utilization threshold exceeded for resource '{resource_name}' on node {node_id}. Current: {current:.2}. Threshold: {threshold:.2}",
        );
        self.send_message(message).await;
    }
}

// Incomplete implementation of https://docs.discord.com/developers/resources/webhook#execute-webhook
#[derive(Serialize, Deserialize)]
pub struct WebhookPayload {
    content: String,
}

impl WebhookPayload {
    /// Construct a new WebhookPayload with simple text content
    pub fn with_content(content: String) -> Self {
        Self { content }
    }
}
