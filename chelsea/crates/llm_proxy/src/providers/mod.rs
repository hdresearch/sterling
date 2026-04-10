//! Provider implementations — passthrough proxies that forward raw request bodies
//! and sniff responses for usage info (spend tracking).

pub mod anthropic_provider;
pub mod openai_provider;

use bytes::Bytes;
use futures::Stream;
use serde_json::Value;
use std::pin::Pin;

use crate::config::ProviderType;
use crate::error::ProviderError;
use crate::types::UsageInfo;

/// Resolved credentials for a provider.
pub struct ProviderCredential {
    pub api_key: Option<String>,
    pub api_base: String,
}

/// Non-streaming response — raw JSON body + extracted usage.
pub struct CompletionResponse {
    pub body: Value,
    pub usage: UsageInfo,
    pub stop_reason: Option<String>,
}

/// Streaming response — raw byte stream + usage extracted after stream ends.
pub struct StreamingResponse {
    pub stream: Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>,
    pub usage_rx: tokio::sync::oneshot::Receiver<UsageInfo>,
}

/// Enum dispatch — avoids async dyn trait issues.
#[derive(Clone, Copy)]
pub enum ProviderImpl {
    OpenAi,
    Anthropic,
}

impl ProviderImpl {
    pub fn from_type(t: &ProviderType) -> Self {
        match t {
            ProviderType::Openai | ProviderType::OpenaiCompatible => Self::OpenAi,
            ProviderType::Anthropic => Self::Anthropic,
        }
    }

    pub async fn send(
        &self,
        client: &reqwest::Client,
        credential: &ProviderCredential,
        model_name: &str,
        body: Value,
    ) -> Result<CompletionResponse, ProviderError> {
        match self {
            Self::OpenAi => openai_provider::send(client, credential, model_name, body).await,
            Self::Anthropic => anthropic_provider::send(client, credential, model_name, body).await,
        }
    }

    pub async fn send_stream(
        &self,
        client: &reqwest::Client,
        credential: &ProviderCredential,
        model_name: &str,
        body: Value,
    ) -> Result<StreamingResponse, ProviderError> {
        match self {
            Self::OpenAi => {
                openai_provider::send_stream(client, credential, model_name, body).await
            }
            Self::Anthropic => {
                anthropic_provider::send_stream(client, credential, model_name, body).await
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }
}
