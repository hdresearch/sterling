//! Integration tests for orchestrator DB repository layer.
//!
//! Each test spins up a Postgres container via testcontainers, runs migrations
//! with dbmate, and exercises the repo methods against real SQL. The seed
//! migration provides a test account, org, API key, orchestrator, and node.

mod db_repos {
    pub mod harness;

    mod api_keys;
    mod commit_tags;
    mod commits;
    mod health_checks;
    mod nodes;
    mod orchestrators;
    mod organizations;
    mod repositories;
    mod usage;
    mod vms;
}
