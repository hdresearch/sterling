# Orchestrator Integration Testing Guide

This guide explains how to write and maintain integration tests for the orchestrator.

## Test Utilities

All common test setup code has been consolidated into `test_utils.rs` to reduce duplication and make tests easier to write.

### Quick Start

```rust
use crate::test_utils::*;

#[tokio::test]
async fn my_new_test() {
    // Skip test if Chelsea endpoint not configured
    skip_if_no_endpoint!();

    // Get complete test environment in one line
    let (db, cluster_id, node_id, test_endpoint, _guard) = setup_action_test().await;

    // Your test code here
    let result = action::call(MyAction::new(cluster_id)).await;
    assert!(result.is_ok());

    // Cleanup (rolls back database transaction)
    db.rollback_for_test().await.unwrap();
}
```

## Available Test Utilities

### Environment Setup

- **`setup_env()`** - Sets required environment variables
- **`get_test_endpoint()`** - Gets Chelsea test endpoint from env
- **`skip_if_no_endpoint!()`** - Macro to skip test if endpoint not set

### Database Helpers

- **`init_test_db()`** - Initialize DB with test transaction
- **`connect_db(url)`** - Create TLS-enabled database connection
- **`ensure_orchestrators_table(client)`** - Ensure required tables exist
- **`get_test_cluster_id(db)`** - Get a seeded cluster ID

### Node Setup

- **`setup_test_node(db, state, endpoint)`** - Create test node with telemetry

### High-Level Setup Functions

- **`setup_action_test()`** - Complete setup for action tests
  - Returns: `(DB, cluster_id, node_id, test_endpoint, guard)`

- **`setup_route_test()`** - Complete setup for route/API tests
  - Returns: `(Router, DB, cluster_id, node_id, test_endpoint, guard)`

### Chelsea Helpers

- **`create_test_vm(endpoint)`** - Create a VM directly via Chelsea

## Before and After Examples

### Before (140+ lines of setup per test file):

```rust
async fn setup() -> (DB, Uuid) {
    dotenv::dotenv().ok();
    unsafe {
        std::env::set_var("REGION", "us-east");
        std::env::set_var("ORCHESTRATOR_IP", "127.0.0.1");
        std::env::set_var("CHELSEA_SSH_KEY_PATH", "/tmp/chelsea-test-key");
    }
    Config::init();

    let url = std::env::var("DATABASE_URL").unwrap();
    let mut b = TlsConnector::builder();
    b.danger_accept_invalid_certs(true);
    let tls = MakeTlsConnector::new(b.build().unwrap());

    let (client, conn) = PgConfig::from_str(&url)
        .unwrap()
        .connect(tls)
        .await
        .unwrap();

    tokio::spawn(async move {
        let _ = conn.await;
    });

    client.simple_query("CREATE TABLE IF NOT EXISTS orchestrators...")
        .await.unwrap();

    let db = DB::new(&url).await.unwrap();
    db.begin_for_test().await.unwrap();

    let cloud = Cloud::with(Region::UsEast).await;
    let state = State::init(Region::UsEast, &db.orchestrator()).await;

    let orchestrator_id = state.orchestrator().id();
    let node_resources = NodeResources::new(4, 8192, 102400, 10);
    let node_entity = db.node().insert(orchestrator_id, &node_resources).await.unwrap();

    // ... 100 more lines ...
}
```

### After (1 line!):

```rust
use crate::test_utils::*;

#[tokio::test]
async fn my_test() {
    skip_if_no_endpoint!();
    let (db, cluster_id, node_id, endpoint, _guard) = setup_action_test().await;

    // Your test here

    db.rollback_for_test().await.unwrap();
}
```

## Running Tests

### Prerequisites

1. **Chelsea Server Running**: Start Chelsea on port 8111
2. **PostgreSQL Database**: Running with seeded data
3. **Environment Variables**:
   ```bash
   export DATABASE_URL="postgresql://postgres:opensesame@127.0.0.1:5432/vers"
   export CHELSEA_TEST_ENDPOINT="127.0.0.1"
   export CHELSEA_SERVER_PORT="8111"
   ```

### Run All Integration Tests

```bash
sudo DATABASE_URL="postgresql://postgres:opensesame@127.0.0.1:5432/vers" \
     CHELSEA_TEST_ENDPOINT=127.0.0.1 \
     CHELSEA_SERVER_PORT=8111 \
     HOME=$HOME \
     cargo test --package orchestrator \
     --features integration-tests \
     --test mod \
     -- --test-threads=1 --nocapture
```

### Run Specific Test

```bash
sudo DATABASE_URL="postgresql://postgres:opensesame@127.0.0.1:5432/vers" \
     CHELSEA_TEST_ENDPOINT=127.0.0.1 \
     CHELSEA_SERVER_PORT=8111 \
     HOME=$HOME \
     cargo test --package orchestrator \
     --features integration-tests \
     --test mod \
     test_new_root_vm_comprehensive \
     -- --test-threads=1 --nocapture
```

### Why `--test-threads=1`?

Tests must run sequentially because they share a database connection pool. Concurrent tests cause prepared statement conflicts during transaction rollback.

### Why `sudo`?

Some tests require elevated permissions for:
- Creating WireGuard interfaces
- Network namespace operations
- System resource access

## Test Structure

```
tests/
├── test_utils.rs          # Centralized test utilities (NEW!)
├── integration/
│   ├── actions/
│   │   ├── common.rs      # Re-exports test_utils
│   │   └── new_root_vm.rs # Action tests
│   ├── routes/
│   │   ├── common.rs      # Re-exports test_utils
│   │   └── vm_routes.rs   # API route tests
│   └── node_proto/
│       ├── common.rs      # Re-exports test_utils
│       └── *.rs           # Protocol tests
```

## Best Practices

1. **Always use `skip_if_no_endpoint!()`** for tests that require Chelsea
2. **Always call `db.rollback_for_test()`** at the end to cleanup
3. **Keep the `_guard` variable in scope** - it manages action context lifecycle
4. **Use descriptive test names** - `test_action_succeeds_with_valid_input`
5. **Add cleanup** - Delete VMs created during tests when possible

## Common Patterns

### Testing an Action

```rust
#[tokio::test]
async fn test_my_action() {
    skip_if_no_endpoint!();
    let (db, cluster_id, _node_id, _endpoint, _guard) = setup_action_test().await;

    let result = action::call(MyAction::new(cluster_id)).await;
    assert!(result.is_ok());

    db.rollback_for_test().await.unwrap();
}
```

### Testing a Route

```rust
#[tokio::test]
async fn test_my_route() {
    skip_if_no_endpoint!();
    let (router, db, cluster_id, _node_id, _endpoint, _guard) = setup_route_test().await;

    let response = router
        .oneshot(Request::builder().uri("/api/v1/vm/new_root").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    db.rollback_for_test().await.unwrap();
}
```

### Creating and Cleaning Up VMs

```rust
#[tokio::test]
async fn test_with_vm_cleanup() {
    skip_if_no_endpoint!();
    let (db, cluster_id, _node_id, _endpoint, _guard) = setup_action_test().await;

    // Create VM
    let vm_result = action::call(NewRootVM::new(request, cluster_id)).await.unwrap();
    let vm_id = Uuid::parse_str(&vm_result.id).unwrap();

    // Test something with the VM
    // ...

    // Cleanup VM before rollback
    let _ = action::call(DeleteVM::new(vm_id, false)).await;

    db.rollback_for_test().await.unwrap();
}
```

## Troubleshooting

### Tests Skip Silently

**Problem**: Tests print "Skipping test: CHELSEA_TEST_ENDPOINT not set"

**Solution**: Set the `CHELSEA_TEST_ENDPOINT` environment variable

### Permission Denied Errors

**Problem**: Tests fail with "Operation not permitted"

**Solution**: Run tests with `sudo`

### Connection Refused

**Problem**: Tests fail to connect to Chelsea or database

**Solution**:
1. Verify Chelsea is running: `curl http://127.0.0.1:8111/health`
2. Verify PostgreSQL is running: `psql $DATABASE_URL -c "SELECT 1"`

### Prepared Statement Errors

**Problem**: Tests fail with "prepared statement does not exist"

**Solution**: Use `--test-threads=1` to run tests sequentially

## Contributing

When adding new integration tests:

1. Use the shared `test_utils.rs` functions
2. Add new common patterns to `test_utils.rs` if they're reusable
3. Document any special setup requirements
4. Ensure tests clean up after themselves
