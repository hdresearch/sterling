pub mod common;
pub mod vm_routes;

#[cfg(test)]
mod readme {
    /// This test serves as documentation for how to run the integration tests
    #[test]
    fn how_to_run_these_tests() {
        eprintln!("\n=== How to Run Orchestrator REST API Route Integration Tests ===\n");
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
        eprintln!("    Due to shared action context singleton, concurrent tests will");
        eprintln!("    cause database transaction conflicts.\n");
        eprintln!("Run all route tests:");
        eprintln!("  sudo CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("       DATABASE_URL=\"postgresql://postgres:opensesame@127.0.0.1:5432/vers\" \\");
        eprintln!("       HOME=/home/ubuntu cargo test \\");
        eprintln!("       --package orchestrator --features integration-tests --test mod \\");
        eprintln!("       integration::routes -- --test-threads=1 --nocapture\n");
        eprintln!("Run just VM route tests (comprehensive):");
        eprintln!("  sudo CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("       DATABASE_URL=\"postgresql://postgres:opensesame@127.0.0.1:5432/vers\" \\");
        eprintln!("       HOME=/home/ubuntu cargo test \\");
        eprintln!("       --package orchestrator --features integration-tests --test mod \\");
        eprintln!("       test_vm_routes_comprehensive -- --test-threads=1 --nocapture\n");
        eprintln!("Run just authentication tests:");
        eprintln!("  sudo CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 \\");
        eprintln!("       DATABASE_URL=\"postgresql://postgres:opensesame@127.0.0.1:5432/vers\" \\");
        eprintln!("       HOME=/home/ubuntu cargo test \\");
        eprintln!("       --package orchestrator --features integration-tests --test mod \\");
        eprintln!("       test_authentication -- --test-threads=1 --nocapture\n");
        eprintln!("Note: Tests will be skipped if CHELSEA_TEST_ENDPOINT is not set.\n");
        eprintln!("⚠️  WARNING: These tests will create and delete VMs on the Chelsea server!");
        eprintln!("    Make sure you're running against a test instance, not production!\n");
        eprintln!("What these tests validate:");
        eprintln!("  ✓ HTTP request/response handling");
        eprintln!("  ✓ Route parameter parsing (path params, query params, body)");
        eprintln!("  ✓ JSON serialization/deserialization");
        eprintln!("  ✓ Authentication middleware");
        eprintln!("  ✓ Error handling and HTTP status codes (400, 404, 500)");
        eprintln!("  ✓ Full stack integration (routes → actions → proto → Chelsea server)\n");
    }
}
