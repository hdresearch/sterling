//! # Chelsea Integration Test Framework
//!
//! Provides a test environment that exposes VmManager - the same interface
//! the production Chelsea web server uses.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use chelsea_test::run_test;
//!
//! #[test]
//! fn test_vm_lifecycle() {
//!     run_test(|env| async move {
//!         let vm_manager = env.vm_manager();
//!         // ... test code
//!         Ok(())
//!     });
//! }
//! ```
//!
//! ## Requirements
//!
//! Tests require:
//! - Root/CAP_NET_ADMIN for networking
//! - Ceph cluster access
//! - Docker (for PostgreSQL testcontainer)

mod logging;

use std::future::Future;
use std::net::Ipv4Addr;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chelsea_lib::network_manager::manager::VmNetworkManager;
use chelsea_lib::network_manager::network_ranges::NetworkRanges;
use chelsea_lib::network_manager::store::VmNetworkManagerStore;
use chelsea_lib::process_manager::VmProcessManager;
use chelsea_lib::ready_service::VmReadyService;
use chelsea_lib::s3_store::S3SnapshotStore;
use chelsea_lib::system_service::SystemService;
use chelsea_lib::vm_manager::VmManager;
use chelsea_lib::volume_manager::VmVolumeManager;
use chelsea_lib::volume_manager::ceph::CephVmVolumeManager;
use ipnet::Ipv4Net;
use tempfile::TempDir;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::runtime::Builder;
use util::linux::get_primary_network_interface;
use vers_config::VersConfig;
use vers_pg::db::VersPg;

/// Test environment exposing VmManager.
pub struct TestEnv {
    vm_manager: Arc<VmManager>,
    _temp_dir: TempDir,
    _pg_container: ContainerAsync<Postgres>,
}

impl TestEnv {
    /// Get the VmManager.
    pub fn vm_manager(&self) -> &Arc<VmManager> {
        &self.vm_manager
    }
}

/// Run a test with the full test environment.
///
/// Sets up PostgreSQL, SQLite, Ceph, and networking.
/// Requires root/CAP_NET_ADMIN and Ceph access.
#[track_caller]
pub fn run_test<F, Fut>(test_fn: F)
where
    F: FnOnce(TestEnv) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    logging::init_test_logging();

    let rt = Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
        .expect("Failed to build tokio runtime");

    rt.block_on(async move {
        match create_test_env().await {
            Ok(env) => {
                if let Err(e) = test_fn(env).await {
                    panic!("Test failed: {:#}", e);
                }
            }
            Err(e) => {
                panic!("Failed to create test environment: {:#}", e);
            }
        }
    });
}

async fn create_test_env() -> Result<TestEnv> {
    let temp_dir = TempDir::new()?;

    // SQLite database
    let db_path = temp_dir.path().join("chelsea.db");
    let sqlite = Arc::new(chelsea_db::ChelseaDb::new_at_path(&db_path).await?);

    // PostgreSQL container
    let (pg, container) = create_postgres().await?;
    let pg = Arc::new(pg);

    let process_manager = Arc::new(VmProcessManager::new(sqlite.clone(), pg.clone()));
    let system_service = Arc::new(SystemService::default());
    let config = VersConfig::chelsea();

    // Network manager
    let network_manager: Arc<VmNetworkManager> = {
        let store: Arc<dyn VmNetworkManagerStore> = sqlite.clone();
        let test_subnet = Ipv4Net::new(Ipv4Addr::new(192, 168, 200, 0), 30)?;
        let test_ports = 29000..29002u16;

        let ranges = NetworkRanges::new(test_subnet, test_ports)?;
        let network_interface = get_primary_network_interface().await?;
        let nm = VmNetworkManager::new(network_interface, ranges, store)
            .await
            .context("Failed to create network manager")?;

        nm.initialize_networks()
            .await
            .context("Failed to initialize networks")?;

        Arc::new(nm)
    };

    // Volume manager (real Ceph)
    let ceph_volume_manager = CephVmVolumeManager::new(sqlite.clone())
        .await
        .context("Failed to create Ceph volume manager")?;
    ceph_volume_manager.start_pool().await;
    let volume_manager: Arc<dyn VmVolumeManager> = Arc::new(ceph_volume_manager);

    // Snapshot store
    let snapshot_dir = config.snapshot_dir.clone();
    std::fs::create_dir_all(&snapshot_dir)?;

    let s3_config = aws_sdk_s3::Config::builder()
        .endpoint_url("http://localhost:9000")
        .region(aws_sdk_s3::config::Region::new("us-east-1"))
        .credentials_provider(aws_sdk_s3::config::Credentials::new(
            "test", "test", None, None, "test",
        ))
        .behavior_version_latest()
        .build();
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config);

    let snapshot_store = Arc::new(
        S3SnapshotStore::new(snapshot_dir, config.snapshot_dir_max_size_mib, s3_client).await?,
    );

    // Ready service
    let ready_service = Arc::new(VmReadyService::new(
        sqlite.clone(),
        format!(
            "http://127.0.0.1:{}/boot/{{vm_id}}",
            config.event_server_port
        ),
    ));

    let vm_manager = Arc::new(VmManager {
        local_store: sqlite,
        remote_store: pg,
        network_manager,
        process_manager,
        system_service,
        volume_manager,
        commit_store: snapshot_store.clone(),
        sleep_snapshot_store: snapshot_store,
        ready_service,
        vm_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
        hypervisor_type: config.hypervisor_type,
    });

    Ok(TestEnv {
        vm_manager,
        _temp_dir: temp_dir,
        _pg_container: container,
    })
}

async fn create_postgres() -> Result<(VersPg, ContainerAsync<Postgres>)> {
    use std::process::Command;
    use testcontainers::runners::AsyncRunner;

    let container = Postgres::default()
        .start()
        .await
        .context("Failed to start PostgreSQL container")?;

    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;

    let admin_url = format!(
        "postgresql://postgres:postgres@{}:{}/postgres?sslmode=disable",
        host, port
    );
    let vers_url = format!(
        "postgresql://postgres:postgres@{}:{}/vers?sslmode=disable",
        host, port
    );

    // Create vers database
    let output = Command::new("psql")
        .arg(&admin_url)
        .arg("-c")
        .arg("CREATE DATABASE vers;")
        .output()
        .context("Failed to run psql")?;

    if !output.status.success() {
        bail!(
            "Failed to create vers database: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Run migrations
    let pg_dir = find_pg_dir()?;
    let output = Command::new("dbmate")
        .arg("--url")
        .arg(&vers_url)
        .arg("--migrations-dir")
        .arg("./migrations")
        .arg("--no-dump-schema")
        .arg("up")
        .arg("--strict")
        .current_dir(&pg_dir)
        .output()
        .context("Failed to run dbmate")?;

    if !output.status.success() {
        bail!(
            "dbmate failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let db = VersPg::new_with_url(&vers_url, false).await?;
    Ok((db, container))
}

fn find_pg_dir() -> Result<std::path::PathBuf> {
    for candidate in ["pg", "../../pg", "../../../pg"] {
        let path = std::path::PathBuf::from(candidate);
        if path.join("migrations").exists() {
            return Ok(path);
        }
    }

    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let root = std::path::PathBuf::from(manifest_dir)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        if let Some(root) = root {
            let pg_dir = root.join("pg");
            if pg_dir.join("migrations").exists() {
                return Ok(pg_dir);
            }
        }
    }

    bail!("Could not find pg/migrations directory")
}
