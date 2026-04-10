use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use billing::db::BillingDb;
use billing::stripe::{balance, client, meter};
use llm_proxy::api::spend_tracking::CustomerIdCache;
use llm_proxy::config::AppConfig;
use llm_proxy::db::LogDb;
use llm_proxy::routing::ModelRouter;
use llm_proxy::{AppState, api, request_id};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "llm_proxy.toml".to_string());
    let config = AppConfig::load(&config_path)?;

    let listen_addr = format!("{}:{}", config.server.host, config.server.port);

    tracing::info!("connecting to billing database...");
    let billing = BillingDb::connect(&config.database.url)
        .await
        .map_err(|e| format!("billing DB connect failed: {e}"))?;
    tracing::info!("billing database connected, testing...");
    if !billing.ping().await {
        tracing::warn!("billing database ping failed (non-fatal, continuing)");
    }

    tracing::info!("connecting to log database...");
    let logs = LogDb::connect(config.database.log_url())
        .await
        .map_err(|e| format!("log DB connect failed: {e}"))?;
    tracing::info!("log database connected, testing...");
    if !logs.ping().await {
        tracing::warn!("log database ping failed (non-fatal, continuing)");
    }

    // In dev, --migrate creates billing tables (prod uses dbmate).
    // Log tables are always migrated at startup (separate DB, no dbmate).
    if std::env::args().any(|a| a == "--migrate") {
        tracing::info!("running billing migrations (dev mode)");
        billing
            .migrate()
            .await
            .map_err(|e| format!("billing migration failed: {e}"))?;
    }
    tracing::info!("running log database migrations...");
    logs.migrate()
        .await
        .map_err(|e| format!("log DB migration failed: {e}"))?;
    logs.ensure_partitions(3).await?;
    logs.spawn_partition_manager(3);

    let model_router = ModelRouter::from_config(&config)?;

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    // ─── Stripe billing integration (optional) ──────────────────────
    let (meter_sender, balance_cache) =
        match config.stripe.as_ref().and_then(|s| s.secret_key.as_ref()) {
            Some(secret_key) => {
                let stripe_config = config.stripe.as_ref().expect("stripe config present");
                tracing::info!(
                    "stripe billing enabled (meter: {})",
                    stripe_config.meter_event_name
                );

                let stripe_client = client::StripeClient::new(secret_key)
                    .map_err(|e| format!("failed to create Stripe client: {e}"))?;

                let meter_sender = meter::spawn_meter_task(
                    stripe_client.clone(),
                    stripe_config.meter_event_name.clone(),
                    std::time::Duration::from_secs(stripe_config.meter_flush_interval_secs),
                );

                let cache = balance::BalanceCache::new();
                balance::spawn_balance_poller(
                    stripe_client,
                    billing.clone(),
                    cache.clone(),
                    std::time::Duration::from_secs(stripe_config.balance_poll_interval_secs),
                );

                (Some(meter_sender), Some(cache))
            }
            None => {
                tracing::info!("stripe billing not configured, using local credit ledger");
                (None, None)
            }
        };

    let state = Arc::new(AppState {
        billing,
        logs,
        config,
        router: model_router,
        http_client,
        meter: meter_sender,
        balance_cache,
        customer_id_cache: CustomerIdCache::new(),
    });

    let app = Router::new()
        .merge(api::openai::routes(state.clone()))
        .merge(api::anthropic::routes(state.clone()))
        .merge(api::models::routes())
        .merge(api::admin::routes(state.clone()))
        .merge(api::key_exchange::routes(state.clone()))
        .merge(api::health::routes())
        .layer(axum::middleware::from_fn(request_id::request_id_middleware))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10 MiB
        .with_state(state);

    tracing::info!("llm_proxy listening on {listen_addr}");
    let listener = TcpListener::bind(&listen_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("llm_proxy shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("failed to install Ctrl+C handler: {e}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => tracing::error!("failed to install SIGTERM handler: {e}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C, starting graceful shutdown"),
        _ = terminate => tracing::info!("received SIGTERM, starting graceful shutdown"),
    }
}
