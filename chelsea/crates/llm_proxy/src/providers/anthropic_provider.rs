//! Anthropic Messages API passthrough.
//! Forwards request body as-is, sniffs usage from response.

use futures::StreamExt;
use serde_json::Value;

use super::{CompletionResponse, ProviderCredential, StreamingResponse};
use crate::error::ProviderError;
use crate::types::UsageInfo;

const PROVIDER: &str = "anthropic";
const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub async fn send(
    client: &reqwest::Client,
    credential: &ProviderCredential,
    model_name: &str,
    mut body: Value,
) -> Result<CompletionResponse, ProviderError> {
    body["model"] = Value::String(model_name.to_string());
    body["stream"] = Value::Bool(false);

    let base = if credential.api_base.is_empty() {
        ANTHROPIC_API_BASE
    } else {
        &credential.api_base
    };
    let url = format!("{}/v1/messages", base.trim_end_matches('/'));

    let mut req = client
        .post(&url)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body);

    if let Some(ref api_key) = credential.api_key {
        req = req.header("x-api-key", api_key);
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
    let stop_reason = resp_body["stop_reason"].as_str().map(|s| s.to_string());

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

    let base = if credential.api_base.is_empty() {
        ANTHROPIC_API_BASE
    } else {
        &credential.api_base
    };
    let url = format!("{}/v1/messages", base.trim_end_matches('/'));

    let mut req = client
        .post(&url)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body);

    if let Some(ref api_key) = credential.api_key {
        req = req.header("x-api-key", api_key);
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
    let mut accumulated = UsageInfo::default();

    let byte_stream = resp.bytes_stream();
    let stream = byte_stream.map(move |chunk| match chunk {
        Ok(bytes) => {
            if usage_tx.is_some() {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    for line in text.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if let Ok(event) = serde_json::from_str::<Value>(data) {
                                match event["type"].as_str().unwrap_or("") {
                                    "message_start" => {
                                        accumulated.prompt_tokens =
                                            event["message"]["usage"]["input_tokens"]
                                                .as_u64()
                                                .unwrap_or(0)
                                                as u32;
                                    }
                                    "message_delta" => {
                                        accumulated.completion_tokens =
                                            event["usage"]["output_tokens"].as_u64().unwrap_or(0)
                                                as u32;
                                    }
                                    "message_stop" => {
                                        accumulated.total_tokens = accumulated.prompt_tokens
                                            + accumulated.completion_tokens;
                                        let _ =
                                            usage_tx.take().map(|t| t.send(accumulated.clone()));
                                    }
                                    _ => {}
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
    let input = body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
    let output = body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
    UsageInfo {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: input + output,
    }
}
