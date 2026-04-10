use futures_util::FutureExt;
use orch_wg::WG;
use orchestrator::{
    db::{OrchestratorEntity, OrchestratorsRepository},
    inbound::{Inbound, InboundState},
};
use std::{future::Future, panic, sync::Arc};
use tracing_subscriber::EnvFilter;
use vers_config::VersConfig;
use vers_pg::db::VersPg;

pub use mime;
use testcontainers::{runners::AsyncRunner, ContainerAsync};
pub mod client;

pub use orchestrator::{action, db, db::DB};
use testcontainers_modules::postgres::Postgres;
use tokio::runtime::Builder;

pub struct ActionTestEnv {
    _db: DB,
    _vers_pg: Arc<VersPg>,
    _pg_container: ContainerAsync<Postgres>,
    wg: Option<WG>,
    pub orch: OrchestratorEntity,

    #[cfg(feature = "with-chelsea")]
    chelsea_endpoint: (IpAddr, u16),
}

impl ActionTestEnv {
    /// Access the DB bound to this env.
    pub fn db(&self) -> &DB {
        &self._db
    }

    pub fn wg(&self) -> &WG {
        self.wg
            .as_ref()
            .expect("WG not available in db-only test env")
    }

    pub fn inbound(&self) -> axum::Router {
        let state = InboundState::new(self._db.clone(), self._vers_pg.clone());
        Inbound::get_routes(state)
    }

    #[cfg(feature = "with-chelsea")]
    pub fn chelsea_endpoint(&self) -> String {
        let ip = self.chelsea_endpoint.0.to_string();
        let port = self.chelsea_endpoint.1.to_string();
        format!("http://{ip}:{port}")
    }

    pub fn orch_apikey(&self) -> &str {
        // FIXME: some smart logic here.
        "ef90fd52-66b5-47e7-b7dc-e73c4381028fbfa85827e1f1ebab3078c3d3249a72647aef57451bd5feac7b727dcb5842590c"
    }

    /// Runs a test closure with a freshly initialized ActionTestEnv and guarantees
    /// proper graceful shutdown and DB handling, even if
    /// the closure panics.
    pub fn with_env<F, Fut>(f: F)
    where
        F: FnOnce(&'static ActionTestEnv) -> Fut,
        Fut: Future + Send + 'static,
    {
        Self::init_logging();
        let fut = async move {
            let env: *mut ActionTestEnv = Box::into_raw(Box::new(Self::new_env().await));

            // SAFETY: This is done to remove the requirement of moving env into the spawned task.
            let result = tokio::spawn(f(unsafe { &*(env as *const _) }).map(drop)).await;

            // SAFETY: After the reference no longer can be accessed by the task it's re-converted into a
            // non-ptr ActionTestEnv. Even if 'env' is no longer used this must run, otherwise
            // memory leaks happen.
            let env = unsafe { Box::from_raw(env) };

            async fn drop_stuff() {
                action::graceful_teardown().await;
            }

            // task panicked
            match result {
                Ok(_) => {
                    env._pg_container
                        .stop()
                        .await
                        .expect("Failed to stop container");
                    env._pg_container
                        .rm()
                        .await
                        .expect("Failed to remove the container");

                    drop_stuff().await;
                }
                Err(err) => {
                    drop_stuff().await;

                    eprintln!(
                        "\n\nSince test failed, containers associated is being kept for debugging:\npg_container_id: {pg_id}",
                        pg_id = env._pg_container.id()
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

    fn init_logging() {
        const ENV_FILTER: &[(&str, &str)] = &[
            ("testcontainers", "trace"),
            ("bollard", "info"),
            ("aws_runtime", "info"),
            ("aws_smithy_runtime", "info"),
            ("aws_smithy_runtime_api", "info"),
            ("aws_sdk_sts", "info"),
            ("rustls", "info"),
            ("tokio_postgres", "info"),
            ("hyper_util", "debug"),
            ("defguard_wireguard_rs", "debug"),
            ("reqwest", "debug"),
        ];

        let env_filter_str = format!(
            "trace,{}",
            ENV_FILTER
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<String>>()
                .join(",")
        );

        if let Err(err) = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(&env_filter_str))
            .try_init()
        {
            println!("error whilst setting tracing_subscriber: {err:#?}");
        };
    }

    /// Like `with_env`, but skips WireGuard setup. Tests using this
    /// can run without root. Only suitable for actions that don't
    /// touch WireGuard (e.g. ChooseNode, auth, DB-only actions).
    pub fn with_env_no_wg<F, Fut>(f: F)
    where
        F: FnOnce(&'static ActionTestEnv) -> Fut,
        Fut: Future + Send + 'static,
    {
        Self::init_logging();
        let fut = async move {
            let env: *mut ActionTestEnv = Box::into_raw(Box::new(Self::new_env_db_only().await));

            // SAFETY: same as with_env — ref is valid for the spawned task's lifetime.
            let result = tokio::spawn(f(unsafe { &*(env as *const _) }).map(drop)).await;
            let env = unsafe { Box::from_raw(env) };

            async fn drop_stuff() {
                action::graceful_teardown().await;
            }

            match result {
                Ok(_) => {
                    env._pg_container
                        .stop()
                        .await
                        .expect("Failed to stop container");
                    env._pg_container
                        .rm()
                        .await
                        .expect("Failed to remove the container");
                    drop_stuff().await;
                }
                Err(err) => {
                    drop_stuff().await;
                    eprintln!(
                        "\n\nSince test failed, containers associated is being kept for debugging:\npg_container_id: {pg_id}",
                        pg_id = env._pg_container.id()
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

    async fn new_env_db_only() -> ActionTestEnv {
        unsafe { std::env::set_var("TESTCONTAINERS_COMMAND", "keep") };

        let (db, pg_container, vers_pg) = Self::setup_database().await;

        let orch_entity = db
            .orchestrator()
            .get_by_region("us-east")
            .await
            .unwrap()
            .unwrap();

        action::setup_db_only(db.clone(), orch_entity.clone(), vers_pg.clone());

        ActionTestEnv {
            _db: db,
            _vers_pg: vers_pg,
            _pg_container: pg_container,
            orch: orch_entity,
            wg: None,
            #[cfg(feature = "with-chelsea")]
            chelsea_endpoint: (config.chelsea_ip, config.chelsea_port),
        }
    }

    async fn new_env() -> ActionTestEnv {
        // NOTE: https://docs.rs/testcontainers/0.25.0/src/testcontainers/core/env/config.rs.html#239-249
        unsafe { std::env::set_var("TESTCONTAINERS_COMMAND", "keep") };

        let (db, pg_container, vers_pg) = Self::setup_database().await;

        // FIXME: magic value.
        let orch_entity = db
            .orchestrator()
            .get_by_region("us-east")
            .await
            .unwrap()
            .unwrap();

        let wg = WG::new(
            "wgorchestrator",
            orch_entity.wg_ipv6(),
            orch_entity.wg_private_key().to_owned(),
            VersConfig::orchestrator().wg_port,
        )
        .unwrap();

        action::setup(wg.clone(), db.clone(), orch_entity.clone(), vers_pg.clone());

        ActionTestEnv {
            _db: db,
            _vers_pg: vers_pg,
            _pg_container: pg_container,
            orch: orch_entity,
            wg: Some(wg),
            #[cfg(feature = "with-chelsea")]
            chelsea_endpoint: (config.chelsea_ip, config.chelsea_port),
        }
    }

    async fn setup_database() -> (DB, ContainerAsync<Postgres>, Arc<VersPg>) {
        use std::process::Command;

        // Check if dbmate is installed
        let dbmate_check = Command::new("dbmate").arg("--version").output();

        match dbmate_check {
            Ok(output) if output.status.success() => {
                // dbmate is available, continue
            }
            _ => {
                panic!(
                    "dbmate is not installed or not available in PATH.\n\
                     Please install dbmate: https://github.com/amacneil/dbmate\n\
                     On macOS: brew install dbmate\n\
                     On other systems, see the installation guide at the link above."
                );
            }
        }

        // Start a test container for isolation
        let pg = Postgres::default()
            .start()
            .await
            .expect("failed to start test Postgres container");

        let host = pg.get_host().await.expect("host");
        let port = pg.get_host_port_ipv4(5432).await.expect("port");

        // Use the same database URL format as the setup script, but with container host/port
        let url = format!("postgresql://postgres:postgres@{host}:{port}/vers?sslmode=disable");

        // Use dbmate to run migrations like the setup script does
        let output = Command::new("dbmate")
            .arg("--url")
            .arg(&url)
            .arg("--migrations-dir")
            .arg("./migrations")
            .arg("--no-dump-schema")
            .arg("up")
            .arg("--strict")
            .current_dir("../../pg")
            .output()
            .expect("Failed to execute dbmate - make sure dbmate is installed");

        if !output.status.success() {
            panic!(
                "dbmate migration failed with status: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let db = orchestrator::db::DB::new_with_tls(&url, false)
            .await
            .expect("failed to connect to containerized Postgres");

        let vers_pg = Arc::new(
            VersPg::new_with_url(&url, false)
                .await
                .expect("Failed to initialize VersPg for tests"),
        );

        (db, pg, vers_pg)
    }
}
