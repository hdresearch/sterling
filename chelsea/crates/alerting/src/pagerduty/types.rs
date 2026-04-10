use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
pub struct AlertEvent {
    pub routing_key: String,
    pub event_action: EventAction,
    pub dedup_key: Option<String>,
    pub payload: AlertEventPayload,
    pub images: Option<Vec<Image>>,
    pub links: Option<Vec<Link>>,
}

impl Default for AlertEvent {
    fn default() -> Self {
        Self {
            routing_key: "invalid".to_string(),
            event_action: EventAction::Trigger,
            dedup_key: None,
            payload: AlertEventPayload::default(),
            images: None,
            links: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventAction {
    Trigger,
    Acknowledge,
    Resolve,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AlertEventPayload {
    pub summary: String,
    pub source: String,
    pub severity: Severity,
    pub timestamp: Option<DateTime<Utc>>,
    pub component: Option<String>,
    pub group: Option<String>,
    pub class: Option<String>,
    pub custom_details: Option<Value>,
}

impl Default for AlertEventPayload {
    fn default() -> Self {
        Self {
            summary: "Unspecified alert".to_string(),
            source: "unspecified-source".to_string(),
            severity: Severity::Info,
            timestamp: Some(chrono::Utc::now()),
            component: None,
            group: None,
            class: None,
            custom_details: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Error,
    Info,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    pub src: String,
    pub href: Option<String>,
    pub alt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Link {
    pub href: String,
    pub text: Option<String>,
}
