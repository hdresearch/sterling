//! Shared test harness for llm_proxy integration tests.
//! Spins up a Postgres testcontainer, runs migrations, and provides BillingDb + LogDb handles.
//! In tests both point at the same Postgres instance for simplicity.
//!
//! Pattern borrowed from orchestrator's orch_test crate.
//! Run with: cargo nextest run -p llm_proxy

use std::future::Future;
use std::panic;

use futures_util::FutureExt;
use rust_decimal_macros::dec;
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::runtime::Builder;
use uuid::Uuid;

use llm_proxy::db::{BillingDb, LogDb};

pub struct TestEnv {
    pub billing: BillingDb,
    pub logs: LogDb,
    _pg_container: ContainerAsync<Postgres>,
}

/// A team created by [`TestEnv::create_funded_team`].
#[allow(dead_code)]
pub struct TestTeam {
    pub id: Uuid,
}

impl TestEnv {
    /// Run a test with a fresh Postgres container.
    /// Each test gets its own DB — nextest runs each test in its own process.
    pub fn with_env<F, Fut>(f: F)
    where
        F: FnOnce(&'static TestEnv) -> Fut,
        Fut: Future + Send + 'static,
    {
        let fut = async move {
            let env: *mut TestEnv = Box::into_raw(Box::new(Self::setup().await));

            // SAFETY: ref is valid for the spawned task's lifetime, cleaned up below.
            let result = tokio::spawn(f(unsafe { &*(env as *const _) }).map(drop)).await;
            let env = unsafe { Box::from_raw(env) };

            match result {
                Ok(_) => {
                    env._pg_container.stop().await.expect("failed to stop pg");
                    env._pg_container.rm().await.expect("failed to rm pg");
                }
                Err(err) => {
                    eprintln!(
                        "\nTest failed — keeping container for debugging: pg_id={}",
                        env._pg_container.id()
                    );
                    panic::resume_unwind(err.into_panic());
                }
            }
        };

        Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()
            .unwrap()
            .block_on(fut);
    }

    /// Create a team with an initial credit balance.
    /// Keys assigned to this team will pass the team-level credit check.
    #[allow(dead_code)]
    pub async fn create_funded_team(&self, credits: rust_decimal::Decimal) -> TestTeam {
        let id = Uuid::new_v4();
        self.billing
            .create_team(id, "test-team", None)
            .await
            .expect("failed to create test team");
        if credits > dec!(0) {
            self.billing
                .add_team_credits(id, credits, "test setup", None, "test")
                .await
                .expect("failed to add team credits");
        }
        TestTeam { id }
    }

    async fn setup() -> TestEnv {
        unsafe { std::env::set_var("TESTCONTAINERS_COMMAND", "keep") };

        let pg = Postgres::default()
            .start()
            .await
            .expect("failed to start Postgres container");

        let host = pg.get_host().await.expect("host");
        let port = pg.get_host_port_ipv4(5432).await.expect("port");
        let url = format!("postgresql://postgres:postgres@{host}:{port}/postgres?sslmode=disable");

        // In tests, both billing and log DB point at the same Postgres.
        // In production they're separate databases.
        let billing = BillingDb::connect(&url)
            .await
            .expect("failed to connect to test Postgres (billing)");
        billing
            .migrate()
            .await
            .expect("failed to run billing migrations");

        let logs = LogDb::connect(&url)
            .await
            .expect("failed to connect to test Postgres (logs)");
        logs.migrate().await.expect("failed to run log migrations");

        TestEnv {
            billing,
            logs,
            _pg_container: pg,
        }
    }
}
