//! Smoke tests against real LLM providers.
//!
//! These are NOT run in CI — they require real API keys and cost real money.
//! Run manually:
//!
//!   OPENAI_API_KEY=sk-... ANTHROPIC_API_KEY=sk-ant-... \
//!     cargo nextest run -p llm_proxy --test smoke_real_providers
//!
//! Each test sends a single minimal request (~10 tokens) so cost is negligible.

mod test_harness;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::net::TcpListener;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use llm_proxy::auth;
use llm_proxy::config::*;
use llm_proxy::routing::ModelRouter;
use llm_proxy::{AppState, api, request_id};
use test_harness::TestEnv;

struct SmokeEnv {
    proxy_url: String,
    api_key: String,
    key_id: uuid::Uuid,
    state: Arc<AppState>,
}

async fn setup_smoke(
    env: &TestEnv,
    providers: std::collections::HashMap<String, ProviderConfig>,
    models: std::collections::HashMap<String, ModelConfig>,
) -> SmokeEnv {
    let (raw_key, key_hash, key_prefix) = auth::generate_api_key();
    let key_id = uuid::Uuid::new_v4();
    env.billing
        .create_api_key(
            key_id,
            &key_hash,
            &key_prefix,
            Some("smoke"),
            None,
            None,
            None,
            &[],
            None,
        )
        .await
        .unwrap();
    env.billing
        .add_credits(key_id, dec!(10.0), "smoke test", None, "test")
        .await
        .unwrap();

    let config = AppConfig {
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            admin_api_key: Some("smoke-admin".to_string()),
        },
        database: DatabaseConfig {
            url: "unused".to_string(),
            log_url: None,
        },
        providers,
        models,
    };

    let router = ModelRouter::from_config(&config).unwrap();
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();

    let state = Arc::new(AppState {
        billing: env.billing.clone(),
        logs: env.logs.clone(),
        config,
        router,
        http_client,
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = axum::Router::new()
        .merge(api::openai::routes(state.clone()))
        .merge(api::anthropic::routes(state.clone()))
        .merge(api::models::routes())
        .merge(api::admin::routes(state.clone()))
        .merge(api::health::routes())
        .layer(axum::middleware::from_fn(request_id::request_id_middleware))
        .with_state(state.clone());

    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(50)).await;

    SmokeEnv {
        proxy_url: format!("http://{addr}"),
        api_key: raw_key,
        key_id,
        state,
    }
}

fn require_env(name: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("SKIPPING: {name} not set");
            std::process::exit(0);
        }
    }
}

// ─── OpenAI ──────────────────────────────────────────────────────────────────

#[test]
fn smoke_openai_chat_completions() {
    require_env("OPENAI_API_KEY");

    TestEnv::with_env(|env| async move {
        let mut providers = std::collections::HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                provider_type: ProviderType::Openai,
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                api_base: None,
            },
        );
        let mut models = std::collections::HashMap::new();
        models.insert(
            "gpt-4o-mini".to_string(),
            ModelConfig {
                routing: vec!["openai".to_string()],
                model_name: None,
            },
        );

        let smoke = setup_smoke(env, providers, models).await;
        let client = reqwest::Client::new();

        // Send a real request
        let resp = client
            .post(format!("{}/v1/chat/completions", smoke.proxy_url))
            .bearer_auth(&smoke.api_key)
            .json(&json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "Reply with exactly one word: hello"}],
                "max_tokens": 10
            }))
            .send()
            .await
            .unwrap();

        let status = resp.status();
        let request_id = resp
            .headers()
            .get("x-request-id")
            .map(|v| v.to_str().unwrap().to_string());
        let body: serde_json::Value = resp.json().await.unwrap();

        eprintln!("OpenAI status: {status}");
        eprintln!("OpenAI request_id: {request_id:?}");
        eprintln!("OpenAI response: {body}");

        assert!(status.is_success(), "OpenAI returned {status}: {body}");
        assert!(body["choices"][0]["message"]["content"].is_string());
        assert!(body["usage"]["total_tokens"].as_u64().unwrap() > 0);
        assert!(request_id.is_some(), "missing x-request-id header");

        // Wait for fire-and-forget spend tracking to complete
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Verify spend was recorded
        let key_row = smoke
            .state
            .billing
            .get_api_key_by_hash(&auth::hash_api_key(&smoke.api_key))
            .await
            .unwrap()
            .unwrap();
        eprintln!("OpenAI spend recorded: ${:.6}", key_row.spend);
        assert!(
            key_row.spend > Decimal::ZERO,
            "spend should be > 0 after a real request"
        );

        // Verify request log was written
        let spend = smoke
            .state
            .logs
            .get_spend_by_key(smoke.key_id, None)
            .await
            .unwrap();
        eprintln!(
            "OpenAI log: {} requests, {} prompt tokens, {} completion tokens",
            spend.request_count, spend.total_prompt_tokens, spend.total_completion_tokens
        );
        assert_eq!(spend.request_count, 1);
        assert!(spend.total_prompt_tokens > 0);
        assert!(spend.total_completion_tokens > 0);
    });
}

// ─── Anthropic ───────────────────────────────────────────────────────────────

#[test]
fn smoke_anthropic_messages() {
    require_env("ANTHROPIC_API_KEY");

    TestEnv::with_env(|env| async move {
        let mut providers = std::collections::HashMap::new();
        providers.insert(
            "anthropic".to_string(),
            ProviderConfig {
                provider_type: ProviderType::Anthropic,
                api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
                api_base: None,
            },
        );
        let mut models = std::collections::HashMap::new();
        models.insert(
            "claude-haiku-4-5-20251001".to_string(),
            ModelConfig {
                routing: vec!["anthropic".to_string()],
                model_name: None,
            },
        );

        let smoke = setup_smoke(env, providers, models).await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{}/v1/messages", smoke.proxy_url))
            .bearer_auth(&smoke.api_key)
            .json(&json!({
                "model": "claude-haiku-4-5-20251001",
                "messages": [{"role": "user", "content": "Reply with exactly one word: hello"}],
                "max_tokens": 10
            }))
            .send()
            .await
            .unwrap();

        let status = resp.status();
        let request_id = resp
            .headers()
            .get("x-request-id")
            .map(|v| v.to_str().unwrap().to_string());
        let body: serde_json::Value = resp.json().await.unwrap();

        eprintln!("Anthropic status: {status}");
        eprintln!("Anthropic request_id: {request_id:?}");
        eprintln!("Anthropic response: {body}");

        assert!(status.is_success(), "Anthropic returned {status}: {body}");
        assert!(body["content"][0]["text"].is_string());
        assert!(body["usage"]["input_tokens"].as_u64().unwrap() > 0);
        assert!(request_id.is_some(), "missing x-request-id header");

        tokio::time::sleep(Duration::from_secs(1)).await;

        let key_row = smoke
            .state
            .billing
            .get_api_key_by_hash(&auth::hash_api_key(&smoke.api_key))
            .await
            .unwrap()
            .unwrap();
        eprintln!("Anthropic spend recorded: ${:.6}", key_row.spend);
        assert!(
            key_row.spend > Decimal::ZERO,
            "spend should be > 0 after a real request"
        );

        let spend = smoke
            .state
            .logs
            .get_spend_by_key(smoke.key_id, None)
            .await
            .unwrap();
        eprintln!(
            "Anthropic log: {} requests, {} prompt tokens, {} completion tokens",
            spend.request_count, spend.total_prompt_tokens, spend.total_completion_tokens
        );
        assert_eq!(spend.request_count, 1);
        assert!(spend.total_prompt_tokens > 0);
        assert!(spend.total_completion_tokens > 0);
    });
}
