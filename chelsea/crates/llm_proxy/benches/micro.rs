//! Microbenchmarks for CPU-bound hot paths.
//!
//! Run: cargo bench -p llm_proxy --bench micro

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::collections::HashMap;

use llm_proxy::auth;
use llm_proxy::config::*;
use llm_proxy::routing::ModelRouter;
use llm_proxy::spend;
use llm_proxy::types::UsageInfo;

// ─── Key hashing ─────────────────────────────────────────────────────────────

fn bench_hash_api_key(c: &mut Criterion) {
    let key = "sk-vers-a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
    c.bench_function("hash_api_key", |b| {
        b.iter(|| auth::hash_api_key(black_box(key)))
    });
}

fn bench_generate_api_key(c: &mut Criterion) {
    c.bench_function("generate_api_key", |b| b.iter(|| auth::generate_api_key()));
}

// ─── Spend calculation ───────────────────────────────────────────────────────

fn bench_calculate_cost_known(c: &mut Criterion) {
    let usage = UsageInfo {
        prompt_tokens: 50_000,
        completion_tokens: 2_000,
        total_tokens: 52_000,
    };
    c.bench_function("calculate_cost/known_model", |b| {
        b.iter(|| spend::calculate_cost(black_box("gpt-4o"), black_box(&usage)))
    });
}

fn bench_calculate_cost_unknown(c: &mut Criterion) {
    let usage = UsageInfo {
        prompt_tokens: 50_000,
        completion_tokens: 2_000,
        total_tokens: 52_000,
    };
    c.bench_function("calculate_cost/unknown_model", |b| {
        b.iter(|| spend::calculate_cost(black_box("some-unknown-model"), black_box(&usage)))
    });
}

// ─── Routing resolution ──────────────────────────────────────────────────────

fn make_router() -> ModelRouter {
    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            provider_type: ProviderType::Openai,
            api_key_env: Some("OPENAI_API_KEY".to_string()),
            api_base: None,
        },
    );
    providers.insert(
        "anthropic".to_string(),
        ProviderConfig {
            provider_type: ProviderType::Anthropic,
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
            api_base: None,
        },
    );

    let mut models = HashMap::new();
    for i in 0..50 {
        models.insert(
            format!("model-{i}"),
            ModelConfig {
                routing: vec!["openai".to_string()],
                model_name: None,
            },
        );
    }
    models.insert(
        "claude-sonnet".to_string(),
        ModelConfig {
            routing: vec!["anthropic".to_string(), "openai".to_string()],
            model_name: Some("claude-sonnet-4-20250514".to_string()),
        },
    );

    let config = AppConfig {
        server: ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 8090,
            admin_api_key: None,
        },
        database: DatabaseConfig {
            url: "postgres://localhost/test".to_string(),
            log_url: None,
        },
        providers,
        models,
    };

    ModelRouter::from_config(&config).unwrap()
}

fn bench_route_resolve_hit(c: &mut Criterion) {
    let router = make_router();
    c.bench_function("route_resolve/hit", |b| {
        b.iter(|| router.resolve(black_box("claude-sonnet")))
    });
}

fn bench_route_resolve_miss(c: &mut Criterion) {
    let router = make_router();
    c.bench_function("route_resolve/miss", |b| {
        b.iter(|| router.resolve(black_box("nonexistent-model")))
    });
}

fn bench_available_models(c: &mut Criterion) {
    let router = make_router();
    c.bench_function("available_models/51_models", |b| {
        b.iter(|| router.available_models())
    });
}

criterion_group!(
    benches,
    bench_hash_api_key,
    bench_generate_api_key,
    bench_calculate_cost_known,
    bench_calculate_cost_unknown,
    bench_route_resolve_hit,
    bench_route_resolve_miss,
    bench_available_models,
);
criterion_main!(benches);
