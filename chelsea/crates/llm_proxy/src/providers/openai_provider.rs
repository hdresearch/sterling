//! OpenAI / OpenAI-compatible passthrough.
//! Forwards request body as-is, sniffs usage from response.

use futures::StreamExt;
use serde_json::Value;

use super::{CompletionResponse, ProviderCredential, StreamingResponse};
use crate::error::ProviderError;
use crate::types::UsageInfo;

const PROVIDER: &str = "openai";

pub async fn send(
    client: &reqwest::Client,
    credential: &ProviderCredential,
    model_name: &str,
    mut body: Value,
) -> Result<CompletionResponse, ProviderError> {
    body["model"] = Value::String(model_name.to_string());
    body["stream"] = Value::Bool(false);

    let url = format!(
        "{}/chat/completions",
        credential.api_base.trim_end_matches('/')
    );

    let mut req = client.post(&url).json(&body);
    if let Some(ref api_key) = credential.api_key {
        req = req.bearer_auth(api_key);
    }

    let resp = req.send().await.map_err(|e| ProviderError::Request {
        provider: PROVIDER,
        reason: e.to_string(),
    })?;
    let status = resp.status();
    let resp_body: Value = resp.json().await.map_err(|e| ProviderError::Parse {
        provider: PROVIDER,
        reason: e.to_string(),
    })?;

    if !status.is_success() {
        return Err(ProviderError::Upstream {
            provider: PROVIDER,
            status: status.as_u16(),
            body: resp_body.to_string(),
        });
    }

    let usage = extract_usage(&resp_body);
    let stop_reason = resp_body["choices"]
        .get(0)
        .and_then(|c| c["finish_reason"].as_str())
        .map(|s| s.to_string());

    Ok(CompletionResponse {
        body: resp_body,
        usage,
        stop_reason,
    })
}

pub async fn send_stream(
    client: &reqwest::Client,
    credential: &ProviderCredential,
    model_name: &str,
    mut body: Value,
) -> Result<StreamingResponse, ProviderError> {
    body["model"] = Value::String(model_name.to_string());
    body["stream"] = Value::Bool(true);
    body["stream_options"] = serde_json::json!({"include_usage": true});

    let url = format!(
        "{}/chat/completions",
        credential.api_base.trim_end_matches('/')
    );

    let mut req = client.post(&url).json(&body);
    if let Some(ref api_key) = credential.api_key {
        req = req.bearer_auth(api_key);
    }

    let resp = req.send().await.map_err(|e| ProviderError::Request {
        provider: PROVIDER,
        reason: e.to_string(),
    })?;
    let status = resp.status();
    if !status.is_success() {
        let error_body = resp.text().await.unwrap_or_default();
        return Err(ProviderError::Upstream {
            provider: PROVIDER,
            status: status.as_u16(),
            body: error_body,
        });
    }

    let (usage_tx, usage_rx) = tokio::sync::oneshot::channel();
    let mut usage_tx = Some(usage_tx);

    let byte_stream = resp.bytes_stream();
    let stream = byte_stream.map(move |chunk| match chunk {
        Ok(bytes) => {
            if usage_tx.is_some() {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    for line in text.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                let _ = usage_tx.take().map(|t| t.send(UsageInfo::default()));
                            } else if let Ok(v) = serde_json::from_str::<Value>(data) {
                                if let Some(u) = v.get("usage") {
                                    let info = UsageInfo {
                                        prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0)
                                            as u32,
                                        completion_tokens: u["completion_tokens"]
                                            .as_u64()
                                            .unwrap_or(0)
                                            as u32,
                                        total_tokens: u["total_tokens"].as_u64().unwrap_or(0)
                                            as u32,
                                    };
                                    let _ = usage_tx.take().map(|t| t.send(info));
                                }
                            }
                        }
                    }
                }
            }
            Ok(bytes)
        }
        Err(e) => {
            let _ = usage_tx.take().map(|t| t.send(UsageInfo::default()));
            Err(ProviderError::Stream {
                provider: PROVIDER,
                reason: e.to_string(),
            })
        }
    });

    Ok(StreamingResponse {
        stream: Box::pin(stream),
        usage_rx,
    })
}

fn extract_usage(body: &Value) -> UsageInfo {
    body.get("usage")
        .map(|u| UsageInfo {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        })
        .unwrap_or_default()
}
