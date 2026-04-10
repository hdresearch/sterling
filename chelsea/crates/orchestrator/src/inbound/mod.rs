use std::time::Duration;

use axum::{Extension, Router};

use reqwest::StatusCode;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer,
};
mod extractors;
pub mod middleware;
pub mod routes;

pub use middleware::OperationId;

use futures_util::FutureExt;
use tokio::{net::TcpListener, sync::oneshot, task};
use utoipa_axum::router::OpenApiRouter;
use vers_config::VersConfig;

use std::sync::Arc;
use vers_pg::db::VersPg;

use crate::{
    db::{DB, OrchestratorEntity},
    inbound::routes::controlplane::controlplane_router,
    tokio_util::TokioTaskGracefulShutdown,
};

/// Optional Stripe billing state (initialized when Stripe is configured).
#[derive(Clone)]
pub struct StripeBillingState {
    pub client: billing::stripe::client::StripeClient,
    pub db: billing::db::BillingDb,
    pub webhook_secret: String,
}

#[derive(Clone)]
pub struct InboundState {
    pub db: DB,
    pub vers_pg: Arc<VersPg>,
    pub stripe_billing: Option<StripeBillingState>,
}

impl InboundState {
    pub fn new(db: DB, vers_pg: Arc<VersPg>) -> Self {
        Self {
            db,
            vers_pg,
            stripe_billing: None,
        }
    }

    /// Set Stripe billing state.
    pub fn with_stripe_billing(mut self, state: StripeBillingState) -> Self {
        self.stripe_billing = Some(state);
        self
    }

    /// Get references to Stripe billing components (if configured).
    pub fn stripe_billing(
        &self,
    ) -> Option<(
        &billing::stripe::client::StripeClient,
        &str,
        &billing::db::BillingDb,
    )> {
        self.stripe_billing
            .as_ref()
            .map(|s| (&s.client, s.webhook_secret.as_str(), &s.db))
    }
}

pub struct Inbound;

impl Inbound {
    pub fn run_with_state(
        orch: OrchestratorEntity,
        state: InboundState,
    ) -> TokioTaskGracefulShutdown {
        let address = orch.wg_ipv6();
        let port = VersConfig::orchestrator().port;

        tracing::info!(address = ?&format!("[{address}]:{port}"), "booting");

        let routes = Self::get_routes(state);

        let (until_send, until) = oneshot::channel();
        let handle = task::spawn(async move {
            let listener = TcpListener::bind((address, port)).await.unwrap();

            if let Err(err) = axum::serve(listener, routes)
                .with_graceful_shutdown(until.map(|r| {
                    tracing::info!("initiated inbound graceful shutdown");
                    drop(r)
                }))
                .await
            {
                tracing::error!(?err, "axum return err");
            };
            tracing::info!("inbound graceful shutdown: done");
        });

        TokioTaskGracefulShutdown {
            sender: until_send,
            task: handle,
            label: Some("inbound"),
        }
    }
    pub fn get_routes(state: InboundState) -> Router {
        let (routes, _openapi) = OpenApiRouter::default()
            .nest("/api/v1", controlplane_router(&state))
            .split_for_parts();

        // Add Swagger UI in debug mode
        #[cfg(debug_assertions)]
        let routes = {
            use utoipa_swagger_ui::SwaggerUi;
            routes.merge(SwaggerUi::new("/docs").url("/api/openapi.json", _openapi))
        };

        // Apply Tower middleware stack
        let app = routes
            .layer(
                ServiceBuilder::new()
                    // Tracing/logging layer - logs requests and responses
                    .layer(TraceLayer::new_for_http())
                    // Timeout layer - abort requests that take too long
                    .layer(TimeoutLayer::with_status_code(
                        StatusCode::REQUEST_TIMEOUT,
                        Duration::from_secs(
                            VersConfig::orchestrator().incoming_request_timeout_secs,
                        ),
                    ))
                    // Compression layer - gzip responses
                    .layer(CompressionLayer::new())
                    // CORS layer - configure cross-origin requests
                    .layer(
                        CorsLayer::new()
                            .allow_origin(tower_http::cors::Any)
                            .allow_methods(tower_http::cors::Any)
                            .allow_headers(tower_http::cors::Any),
                    ),
            )
            // Operation ID middleware - extracts/generates request ID for tracing
            .layer(axum::middleware::from_fn(
                middleware::operation_id_middleware,
            ))
            .layer(Extension(state));

        app
    }
}
