# Chelsea Project Guidelines

## Git Workflow

**Always branch from `next` and open PRs against `next`.** `next` is the default branch (staging). `production` is the production branch. There is no `main` branch.

```bash
git checkout next && git pull origin next
git checkout -b your-branch-name
# ... make changes ...
gh pr create --base next
```

## Error Handling

**Never use `.unwrap()`, `.expect()`, or `.panic!()`** in library or application code. Always use proper error handling with `Result`, `?`, `anyhow`, or `thiserror`.

- Use `?` to propagate errors
- Use `anyhow::Context` / `.context("...")` for adding context to errors
- Use `thiserror` for defining domain-specific error types
- `unwrap()` is only acceptable in tests and build scripts where failure should halt compilation

**Correct:**
```rust
let value = something().context("failed to do something")?;
```

**Incorrect:**
```rust
let value = something().unwrap();
```

## Configuration Values

All configuration values should be accessed through `VersConfig`, not directly from environment variables.

**Correct:**
```rust
use vers_config::VersConfig;

let url = &VersConfig::proxy().pool_manager_url;
let port = VersConfig::proxy().port;
```

**Incorrect:**
```rust
let url = std::env::var("POOL_MANAGER_URL").unwrap();
```

### Adding New Config Values

1. Add the field to the appropriate config struct in `crates/vers_config/src/vers_config.rs`
2. Add the field initialization in the same file (look for where other fields are initialized)
3. Add the value to the INI files in `config/development/` and `config/production/`
4. Access via `VersConfig::proxy()`, `VersConfig::orchestrator()`, `VersConfig::common()`, or `VersConfig::chelsea()`

## Testing

### Integration Tests

Use `cargo nextest run` instead of `cargo test` for integration tests. The orchestrator uses `OnceLock<ActionContext>` (one context per process), so each integration test must run in its own process. `cargo nextest` does this automatically.

```bash
# Run all integration tests for a specific test file
cargo nextest run -p orchestrator --test choose_node

# Run a single test
cargo nextest run -p orchestrator --test choose_node -E 'test(test_name)'
```

### DB-Only Integration Tests

For actions that only need database access (e.g. `ChooseNode`), use `ActionTestEnv::with_env_no_wg` instead of `ActionTestEnv::with_env`. This skips WireGuard setup and doesn't require root:

```rust
#[test]
fn test_something() {
    ActionTestEnv::with_env_no_wg(|env| {
        async move {
            let db = env.db();
            // ... test code
        }
    });
}
```

Actions that use `ctx.proto()` or `ctx.wg()` still need `with_env` (which requires root).
