pub mod common;
mod new_root_vm;

#[cfg(test)]
mod readme {
    /// This test serves as documentation for how to run the integration tests
    #[test]
    fn how_to_run_these_tests() {
        eprintln!("\n=== How to Run Orchestrator Action Integration Tests ===\n");
        eprintln!("These tests require:");
        eprintln!("  1. A running Chelsea server instance");
        eprintln!("  2. A properly configured PostgreSQL database with seed data\n");
        eprintln!("Setup:");
        eprintln!("  1. Start a Chelsea server (e.g., on localhost or a test instance)");
        eprintln!("  2. Set the CHELSEA_TEST_ENDPOINT environment variable");
        eprintln!("     Example: export CHELSEA_TEST_ENDPOINT=127.0.0.1");
        eprintln!("  3. Optionally set CHELSEA_SERVER_PORT (defaults to 8111)");
        eprintln!("     Example: export CHELSEA_SERVER_PORT=8111");
        eprintln!("  4. Set DATABASE_URL to your test database");
        eprintln!("     Example: export DATABASE_URL=postgres://user:pass@localhost/chelsea_test\n");
        eprintln!("⚠️  IMPORTANT: Tests must run sequentially with --test-threads=1");
        eprintln!("    Due to shared database connection pooling, concurrent tests will");
        eprintln!("    cause prepared statement conflicts during transaction rollback.\n");
        eprintln!("Run all action tests:");
        eprintln!("  sudo CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("       DATABASE_URL=\"...\" HOME=$HOME cargo test \\");
        eprintln!("       --package orchestrator --features integration-tests --test mod \\");
        eprintln!("       integration::actions -- --test-threads=1\n");
        eprintln!("Run just new_root_vm tests:");
        eprintln!("  sudo CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("       DATABASE_URL=\"...\" HOME=$HOME cargo test \\");
        eprintln!("       --package orchestrator --features integration-tests --test mod \\");
        eprintln!("       new_root_vm -- --test-threads=1\n");
        eprintln!("Note: Tests will be skipped if CHELSEA_TEST_ENDPOINT is not set.\n");
        eprintln!("⚠️  WARNING: These tests will create and delete VMs on the Chelsea server!");
        eprintln!("    Make sure you're running against a test instance, not production!\n");
    }
}
