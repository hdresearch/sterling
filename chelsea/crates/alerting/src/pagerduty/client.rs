use tracing::error;
use uuid::Uuid;

use crate::{
    pagerduty::types::{AlertEvent, AlertEventPayload, Severity},
    processor::AlertProcessor,
};

const ENDPOINT_SEND_ALERT_EVENT: &str = "https://events.pagerduty.com/v2/enqueue";

/// A PagerDuty API client
pub struct Client {
    http_client: reqwest::Client,
    routing_key: String,
}

impl Client {
    /// Construct a new PagerDuty API client, reusing an existing HTTP client.
    pub fn with_client(client: reqwest::Client, routing_key: String) -> Self {
        Self {
            http_client: client,
            routing_key,
        }
    }

    /// https://developer.pagerduty.com/docs/send-alert-event
    async fn send_alert_event(&self, event: AlertEvent) {
        match self
            .http_client
            .post(ENDPOINT_SEND_ALERT_EVENT)
            .json(&event)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                error!(
                    status = %response.status(),
                    "pagerduty alert request returned non-success status"
                );
            }
            Err(err) => {
                error!(%err, "failed to send pagerduty alert");
            }
        }
    }
}

#[async_trait::async_trait]
impl AlertProcessor for Client {
    async fn billing_subscription_lookup_failed(
        &self,
        org_id: &Uuid,
        customer_id: &str,
        detail: &str,
    ) {
        let custom_details = serde_json::json!({
            "org_id": org_id.to_string(),
            "customer_id": customer_id,
            "error": detail,
        });

        let summary = format!(
            "Billing subscription lookup failed for org {} (customer {}): {}",
            org_id, customer_id, detail
        );

        let payload = AlertEventPayload {
            summary,
            source: "chelsea-orchestrator".to_string(),
            severity: Severity::Error,
            component: Some("usage-forwarder".to_string()),
            group: Some("billing".to_string()),
            class: Some("billing-subscription".to_string()),
            custom_details: Some(custom_details),
            ..Default::default()
        };

        let event = AlertEvent {
            routing_key: self.routing_key.clone(),
            dedup_key: Some(format!(
                "chelsea-billing-subscription-{}-{}",
                org_id, customer_id
            )),
            payload,
            ..Default::default()
        };

        self.send_alert_event(event).await;
    }

    async fn chelsea_resource_threshold_exceeded(
        &self,
        node_id: &Uuid,
        resource_name: &str,
        threshold: f32,
        current: f32,
    ) {
        let custom_details = serde_json::json!({
            "node_id": node_id.to_string(),
            "resource_name": resource_name,
            "threshold": threshold,
            "current": current,
        });

        let payload = AlertEventPayload {
            summary: format!("Resource threshold exceeded on chelsea node {node_id}"),
            source: format!("chelsea-{}", node_id),
            severity: Severity::Warning,
            component: Some("mulberry".to_string()),
            group: Some("chelsea".to_string()),
            class: Some("resource load".to_string()),
            custom_details: Some(custom_details),
            ..Default::default()
        };

        let event = AlertEvent {
            routing_key: self.routing_key.clone(),
            dedup_key: Some(format!("chelsea-resource-threshold-exceeded-{}", node_id)),
            payload,
            ..Default::default()
        };

        self.send_alert_event(event).await;
    }
}
