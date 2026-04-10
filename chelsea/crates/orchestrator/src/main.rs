use orch_wg::{WG, WgPeer, gen_private_key, gen_public_key};
use orchestrator::action;
use orchestrator::bg::BG;
use orchestrator::db::{DB, OrchestratorsRepository};
use orchestrator::inbound::{Inbound, InboundState, StripeBillingState};
use orchestrator::logging::init_logging;
use orchestrator::usage::{forwarder::StripeUsageContext, spawn_usage_task};
use std::sync::Arc;
use vers_config::VersConfig;
use vers_pg::db::VersPg;

#[tokio::main]
async fn main() {
    let _g = init_logging();

    let db_url = &VersConfig::common().database_url;
    let db = DB::new(&db_url).await.unwrap();
    let inbound_db = db.clone();

    let wg = setup_wg(&db).await.expect("Failed to setup wg");

    let orch_entity = db
        .orchestrator()
        .get_by_region("us-east")
        .await
        .unwrap()
        .unwrap();

    let vers_pg = Arc::new(VersPg::new().await.expect("Failed to initialize VersPg"));
    action::setup(wg.clone(), db.clone(), orch_entity.clone(), vers_pg.clone());

    // ─── Stripe billing integration (optional) ──────────────────────
    let mut stripe_usage_ctx: Option<StripeUsageContext> = None;
    let mut inbound_state = InboundState::new(inbound_db, vers_pg);

    let orch_config = VersConfig::orchestrator();
    if let (Some(secret_key), Some(webhook_secret)) = (
        &orch_config.stripe_secret_key,
        &orch_config.stripe_webhook_secret,
    ) {
        match billing::stripe::client::StripeClient::new(secret_key) {
            Ok(stripe_client) => {
                match billing::db::BillingDb::connect(&VersConfig::common().database_url).await {
                    Ok(billing_db) => {
                        tracing::info!("stripe billing enabled for webhooks + usage + auto-topup");

                        inbound_state = inbound_state.with_stripe_billing(StripeBillingState {
                            client: stripe_client.clone(),
                            db: billing_db.clone(),
                            webhook_secret: webhook_secret.clone(),
                        });

                        stripe_usage_ctx = Some(StripeUsageContext {
                            client: stripe_client.clone(),
                            billing_db: billing_db.clone(),
                        });

                        // Spawn auto-topup background task (checks every 60s)
                        billing::stripe::auto_topup::spawn_auto_topup_task(
                            stripe_client,
                            billing_db,
                            std::time::Duration::from_secs(60),
                        );
                    }
                    Err(e) => {
                        tracing::error!(%e, "failed to connect billing DB; stripe disabled");
                    }
                }
            }
            Err(e) => {
                tracing::error!(%e, "failed to create Stripe client; stripe disabled");
            }
        }
    } else {
        tracing::info!("stripe_secret_key/stripe_webhook_secret not configured; stripe disabled");
    }

    let usage_shutdown = spawn_usage_task(db.clone(), orch_entity.clone(), stripe_usage_ctx);

    let inbound_shutdown = Inbound::run_with_state(orch_entity, inbound_state);
    let bg_shutdown = BG::run();

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen to event");

    tracing::info!("initiating orchestrator shutdown");

    tokio::join!(action::graceful_teardown(), inbound_shutdown);

    bg_shutdown.await;
    if let Some(handle) = usage_shutdown {
        handle.await;
    }

    wg.clear();
}

async fn setup_wg(db: &DB) -> Result<WG, ()> {
    let orch = match db
        .orchestrator()
        .get_by_region("us-east".into())
        .await
        .expect("db-error")
    {
        Some(orch) => orch,
        None => {
            let wg_private_key = gen_private_key();
            let wg_public_key = gen_public_key(&wg_private_key).unwrap();

            db.orchestrator()
                .insert(
                    "us-east",
                    VersConfig::orchestrator().public_ip.clone(),
                    wg_private_key,
                    wg_public_key,
                )
                .await
                .unwrap()
        }
    };

    let wg = WG::new_with_peers(
        "wgorchestrator",
        orch.wg_ipv6(),
        orch.wg_private_key().to_string(),
        VersConfig::orchestrator().wg_port,
        vec![WgPeer {
            pub_key: VersConfig::proxy().wg_public_key.clone(),
            port: VersConfig::proxy().wg_port,
            remote_ipv6: VersConfig::proxy().wg_private_ip.clone(),
            endpoint_ip: VersConfig::proxy().public_ip.into(),
        }],
    )
    .expect("orchestrator: Failed to setup wireguard");
    Ok(wg)
}
