//! Shared test harness for DB repo integration tests.
//!
//! Each test spins up a Postgres container via testcontainers, runs migrations
//! with dbmate, and exercises the repo methods against real SQL. The seed
//! migration provides a test account, org, API key, orchestrator, and node.

use chrono::Utc;
use orchestrator::db::*;
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

// Re-export for submodules that need NodeResources, etc.
pub use orchestrator::db::{ChelseaNodeRepository, NodeResources};

pub async fn setup() -> (DB, ContainerAsync<Postgres>) {
    use std::process::Command;

    let pg = Postgres::default()
        .start()
        .await
        .expect("failed to start Postgres container");

    let host = pg.get_host().await.expect("host");
    let port = pg.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgresql://postgres:postgres@{host}:{port}/vers?sslmode=disable");

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
        .expect("dbmate not found — install with: brew install dbmate");

    if !output.status.success() {
        panic!(
            "dbmate migration failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let db = DB::new_with_tls(&url, false)
        .await
        .expect("failed to connect to containerized Postgres");

    // Insert the seed node (not part of the SQL seed migration, but needed by
    // many tests that create VMs referencing seed_node_id).
    db.node()
        .insert(
            seed_node_id(),
            &seed_orch_id(),
            &NodeResources::new(96, 193025, 1000000, 64),
            "seed-node-priv-key",
            "seed-node-pub-key",
            Some("fd00:fe11:deed:0::100".parse().unwrap()),
            Some("10.0.0.1".parse().unwrap()),
        )
        .await
        .expect("failed to insert seed node");

    (db, pg)
}

// Seed data constants (from 20251111063619_seed_db.sql)
pub const SEED_ACCOUNT_ID: &str = "47750c3a-d1fa-4f33-8135-f972fadfe3bd";
pub const SEED_ORG_ID: &str = "2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d";
pub const SEED_USER_ID: &str = "9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9";
pub const SEED_API_KEY_ID: &str = "ef90fd52-66b5-47e7-b7dc-e73c4381028f";
pub const SEED_API_KEY_HASH: &str = "4c3bf6d11ca3cc96e69df912489f9c0ce8b74a4bc8e816342380f7cc2e4a5605776c4959e94bdad3205cbc71c8b5a6cc9f4d1eadf5cb8b03e77a182b7fe5cb9b";
pub const SEED_ORCH_ID: &str = "18e1ecdb-6e6c-4336-868b-29f42f25ea54";
pub const SEED_NODE_ID: &str = "4569f1fe-054b-4e8d-855a-f3545167f8a9";

pub fn seed_orch_id() -> Uuid {
    Uuid::parse_str(SEED_ORCH_ID).unwrap()
}
pub fn seed_node_id() -> Uuid {
    Uuid::parse_str(SEED_NODE_ID).unwrap()
}
pub fn seed_api_key_id() -> Uuid {
    Uuid::parse_str(SEED_API_KEY_ID).unwrap()
}
pub fn seed_org_id() -> Uuid {
    Uuid::parse_str(SEED_ORG_ID).unwrap()
}
pub fn seed_account_id() -> Uuid {
    Uuid::parse_str(SEED_ACCOUNT_ID).unwrap()
}

/// Helper: create a VM + commit so we have a valid commit_id for testing.
pub async fn create_test_commit(db: &DB, suffix: &str) -> (Uuid, Uuid) {
    let vm_id = Uuid::new_v4();
    let commit_id = Uuid::new_v4();
    let ip: std::net::Ipv6Addr = format!("fd00:fe11:deed:1::a{suffix}")
        .parse()
        .unwrap_or_else(|_| format!("fd00:fe11:deed:1::{suffix}").parse().unwrap());

    db.vms()
        .insert(
            vm_id,
            None,
            None,
            seed_node_id(),
            ip,
            format!("priv-{suffix}"),
            format!("pub-{suffix}"),
            52000 + suffix.parse::<u16>().unwrap_or(0),
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    db.commits()
        .insert(
            commit_id,
            Some(vm_id),
            None,
            seed_api_key_id(),
            format!("snapshot-{suffix}"),
            None,
            Utc::now(),
            false,
        )
        .await
        .unwrap();

    (vm_id, commit_id)
}
