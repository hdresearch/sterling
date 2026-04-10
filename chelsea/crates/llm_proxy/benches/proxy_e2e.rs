//! End-to-end proxy benchmarks.
//!
//! Measures real overhead the proxy adds on top of provider latency.
//! Setup: testcontainer Postgres + mock provider (instant responses) + full proxy.
//! Run:   cargo bench -p llm_proxy --bench proxy_e2e

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::runtime::Runtime;

use llm_proxy::auth;
use llm_proxy::config::*;
use llm_proxy::db::{BillingDb, LogDb};
use llm_proxy::routing::ModelRouter;
use llm_proxy::{AppState, api};

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

struct BenchEnv {
    proxy_url: String,
    api_key: String,
    rt: Runtime,
    _pg: Box<dyn std::any::Any + Send + Sync>,
}

static ENV: OnceLock<BenchEnv> = OnceLock::new();

fn get_env() -> &'static BenchEnv {
    ENV.get_or_init(|| {
        let rt = Runtime::new().unwrap();
        let (proxy_url, api_key, pg) = rt.block_on(setup());
        BenchEnv {
            proxy_url,
            api_key,
            rt,
            _pg: pg,
        }
    })
}

async fn start_mock_provider() -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "id": "chatcmpl-bench",
                "object": "chat.completion",
                "model": "gpt-4o",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello!"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), handle)
}

async fn setup() -> (String, String, Box<dyn std::any::Any + Send + Sync>) {
    let pg = Postgres::default().start().await.unwrap();
    let host = pg.get_host().await.unwrap();
    let port = pg.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgresql://postgres:postgres@{host}:{port}/postgres?sslmode=disable");

    let billing = BillingDb::connect(&db_url).await.unwrap();
    billing.migrate().await.unwrap();
    let logs = LogDb::connect(&db_url).await.unwrap();
    logs.migrate().await.unwrap();

    let (raw_key, key_hash, key_prefix) = auth::generate_api_key();
    let key_id = uuid::Uuid::new_v4();
    billing
        .create_api_key(
            key_id,
            &key_hash,
            &key_prefix,
            Some("bench"),
            None,
            None,
            None,
            &[],
            None,
        )
        .await
        .unwrap();
    billing
        .add_credits(key_id, 100_000.0, "bench", None, "bench")
        .await
        .unwrap();

    let (provider_url, _) = start_mock_provider().await;

    let mut providers = std::collections::HashMap::new();
    providers.insert(
        "mock".to_string(),
        ProviderConfig {
            provider_type: ProviderType::OpenaiCompatible,
            api_key_env: None,
            api_base: Some(provider_url),
        },
    );
    let mut models = std::collections::HashMap::new();
    models.insert(
        "gpt-4o".to_string(),
        ModelConfig {
            routing: vec!["mock".to_string()],
            model_name: None,
        },
    );

    let config = AppConfig {
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            admin_api_key: Some("bench-admin".to_string()),
        },
        database: DatabaseConfig {
            url: db_url,
            log_url: None,
        },
        providers,
        models,
    };

    let router = ModelRouter::from_config(&config).unwrap();
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let state = Arc::new(AppState {
        billing,
        logs,
        config,
        router,
        http_client,
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();

    let app = axum::Router::new()
        .merge(api::openai::routes(state.clone()))
        .merge(api::anthropic::routes(state.clone()))
        .merge(api::models::routes())
        .merge(api::admin::routes(state.clone()))
        .merge(api::health::routes())
        .layer(axum::middleware::from_fn(
            llm_proxy::request_id::request_id_middleware,
        ))
        .with_state(state);

    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(50)).await;

    (format!("http://{proxy_addr}"), raw_key, Box::new(pg))
}

// ─── Benchmarks ──────────────────────────────────────────────────────────────

fn bench_all(c: &mut Criterion) {
    let env = get_env();
    let client = reqwest::Client::new();

    // 1. Single request — full path: auth + route + proxy + mock + spend tracking
    c.bench_function("e2e/single_request", |b| {
        b.to_async(&env.rt).iter(|| {
            let client = client.clone();
            let url = format!("{}/v1/chat/completions", env.proxy_url);
            let key = env.api_key.clone();
            async move {
                let resp = client.post(&url).bearer_auth(&key)
                    .json(&json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]}))
                    .send().await.unwrap();
                let status = resp.status();
                let body = resp.bytes().await.unwrap();
                assert!(status.is_success(), "{status}: {}", String::from_utf8_lossy(&body));
            }
        });
    });

    // 2. Auth rejection — isolates auth DB lookup overhead (no provider call)
    c.bench_function("e2e/auth_reject", |b| {
        b.to_async(&env.rt).iter(|| {
            let client = client.clone();
            let url = format!("{}/v1/chat/completions", env.proxy_url);
            async move {
                let resp = client.post(&url).bearer_auth("sk-vers-bogus")
                    .json(&json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]}))
                    .send().await.unwrap();
                assert_eq!(resp.status(), 401);
                let _ = resp.bytes().await.unwrap();
            }
        });
    });

    // 3. Health check — DB ping overhead (both pools)
    c.bench_function("e2e/health_check", |b| {
        b.to_async(&env.rt).iter(|| {
            let client = client.clone();
            let url = format!("{}/health", env.proxy_url);
            async move {
                let resp = client.get(&url).send().await.unwrap();
                assert!(resp.status().is_success());
                let _ = resp.bytes().await.unwrap();
            }
        });
    });

    // 4. Concurrent requests — tests connection pool pressure
    let mut group = c.benchmark_group("e2e/concurrent");
    for n in [1, 10, 50, 100] {
        group.bench_function(format!("{n}"), |b| {
            b.to_async(&env.rt).iter(|| {
                let client = client.clone();
                let url = format!("{}/v1/chat/completions", env.proxy_url);
                let key = env.api_key.clone();
                async move {
                    let futs: Vec<_> = (0..n).map(|_| {
                        let client = client.clone();
                        let url = url.clone();
                        let key = key.clone();
                        tokio::spawn(async move {
                            let resp = client.post(&url).bearer_auth(&key)
                                .json(&json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]}))
                                .send().await.unwrap();
                            assert!(resp.status().is_success());
                            resp.bytes().await.unwrap()
                        })
                    }).collect();
                    for f in futs { f.await.unwrap(); }
                }
            });
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(10))
        .warm_up_time(Duration::from_secs(2));
    targets = bench_all,
}
criterion_main!(benches);
