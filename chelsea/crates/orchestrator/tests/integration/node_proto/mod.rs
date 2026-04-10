mod branch;
mod commit;
pub mod common;
mod create;
mod delete;
mod from_commit;
mod list_all;
mod system_health;
mod system_telemetry;
mod update_state;

#[cfg(test)]
mod readme {
    /// This test serves as documentation for how to run the integration tests
    #[test]
    fn how_to_run_these_tests() {
        eprintln!("\n=== How to Run Chelsea Node Proto Integration Tests ===\n");
        eprintln!("These tests require a running Chelsea server instance.\n");
        eprintln!("Setup:");
        eprintln!("  1. Start a Chelsea server (e.g., on localhost or a test instance)");
        eprintln!("  2. Set the CHELSEA_TEST_ENDPOINT environment variable to the server's IP");
        eprintln!("     Example: export CHELSEA_TEST_ENDPOINT=127.0.0.1");
        eprintln!("  3. Optionally set CHELSEA_SERVER_PORT (defaults to 8111)");
        eprintln!("     Example: export CHELSEA_SERVER_PORT=8111\n");
        eprintln!("Run tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("    cargo test --package orchestrator --test mod integration::node_proto\n");
        eprintln!("Or with URL format:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=http://0.0.0.0:8111 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("    cargo test --package orchestrator --test mod integration::node_proto\n");
        eprintln!("Run specific test:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("    cargo test --package orchestrator test_new_root_vm_success\n");
        eprintln!("Run just create tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::create\n"
        );
        eprintln!("Run just branch tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::branch\n"
        );
        eprintln!("Run just commit tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::commit\n"
        );
        eprintln!("Run just from_commit tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::from_commit\n"
        );
        eprintln!("Run just update_state tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::update_state\n"
        );
        eprintln!("Run just list_all tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::list_all\n"
        );
        eprintln!("Run just delete tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::delete\n"
        );
        eprintln!("Run just system_health tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::system_health\n"
        );
        eprintln!("Run just system_telemetry tests:");
        eprintln!("  CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!(
            "    cargo test --package orchestrator --test mod integration::node_proto::system_telemetry\n"
        );
        eprintln!("Note: Tests will be skipped if CHELSEA_TEST_ENDPOINT is not set.\n");
    }
}
