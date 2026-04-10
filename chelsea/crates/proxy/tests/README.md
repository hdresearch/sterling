# Integration Tests

This directory contains integration tests for the SSH-over-TLS proxy.

## Test Structure

- `common/` - Shared test utilities
  - `mod.rs` - TLS test server, SNI capture, helpers
  - `ssh_container.rs` - Docker-based SSH server for testing
- `sni_extraction.rs` - SNI extraction tests (4 tests)
- `ssh_forwarding.rs` - SSH forwarding tests (4 tests, Docker required)

## Running Tests

### Unit Tests Only (No Docker Required)

```bash
cargo test --lib
```

### SNI Extraction Tests (No Docker Required)

```bash
cargo test --test sni_extraction
```

### SSH Forwarding Tests (Docker Required)

The SSH forwarding tests use Docker containers via testcontainers-rs.

**Prerequisites:**
- Docker daemon running
- `sshpass` installed: `sudo apt-get install sshpass`
- `nc` (netcat) installed: `sudo apt-get install netcat`
- User added to docker group: `sudo usermod -aG docker $USER` (then log out/in)

**Running with sudo (if docker group not active):**

If you encounter "permission denied" errors when connecting to Docker, run tests with sudo:

```bash
# Run all Docker tests with sudo
sudo -E $(which cargo) test -- --ignored --nocapture

# Run specific test with sudo
sudo -E $(which cargo) test test_ssh_container_exec -- --ignored --nocapture
```

**Running without sudo (recommended):**

```bash
# Run ignored tests (includes Docker-based tests)
cargo test --test ssh_forwarding -- --ignored

# Run specific test
cargo test --test ssh_forwarding test_ssh_container_infrastructure -- --ignored --nocapture
```

### All Tests

```bash
# Run all tests except Docker-based ones
cargo test

# Run ALL tests including Docker-based ones
cargo test -- --ignored
```

## Test Categories

### 1. SNI Extraction Tests

Tests that verify SNI hostname extraction from TLS ClientHello:

- ✅ `test_sni_extraction_basic` - Basic SNI capture
- ✅ `test_sni_extraction_multiple_hostnames` - Multiple different hostnames
- ✅ `test_no_sni_graceful_handling` - Handling connections without SNI
- ✅ `test_wildcard_cert_matching` - Wildcard certificate validation

**Status:** All passing, no Docker required

### 2. SSH Forwarding Tests

Tests that verify end-to-end SSH forwarding through the TLS proxy:

- ⏸️ `test_ssh_forwarding_with_container` - Full end-to-end test (Phase 1.4)
- ✅ `test_ssh_container_infrastructure` - Verify test setup
- ⏸️ `test_tcp_forwarding_basic` - Basic TCP forwarding (Phase 1.4)
- 📋 `test_ssh_through_tls_manual` - Manual connection flow docs

**Status:** Infrastructure tests pass, forwarding tests pending Phase 1.4 implementation

## SSH Container Details

The tests use the `linuxserver/openssh-server` Docker image which provides:

- OpenSSH server on port 2222 (exposed)
- Username: `testuser`
- Password: `testpass`
- Minimal Alpine Linux environment
- Quick startup (~2-3 seconds)

## Test Helpers

### `SshContainer`

Manages SSH server containers for testing:

```rust
// Start container
let container = SshContainer::start().await?;

// Test direct connection
let result = container.test_direct_connection().await?;

// Execute commands
let output = container.exec_ssh_command("uname -s").await?;
```

### `CapturedSni`

Thread-safe SNI capture for tests:

```rust
let captured_sni = CapturedSni::new();

// In TLS resolver
captured_sni.set(hostname.to_string());

// In test
let sni = captured_sni.wait_for_sni(1000);
assert_eq!(sni.unwrap(), "expected.hostname.com");
```

## Troubleshooting

### Docker permission denied

```
Error: failed to create a container: Error in the hyper legacy client: client error (Connect)
```

**Solution:** Run tests with sudo or add user to docker group:
```bash
# Option 1: Run with sudo
sudo -E $(which cargo) test -- --ignored --nocapture

# Option 2: Add user to docker group (requires logout/login)
sudo usermod -aG docker $USER
```

### Docker not running

```
Error: Cannot connect to the Docker daemon
```

**Solution:** Start Docker: `sudo systemctl start docker`

### sshpass not found

```
Error: No such file or directory (os error 2)
```

**Solution:** Install sshpass: `sudo apt-get install sshpass`

### Too many authentication failures

```
Error: SSH command failed: Warning: Permanently added '[127.0.0.1]:32811' (ED25519) to the list of known hosts.
Received disconnect from 127.0.0.1 port 32811:2: Too many authentication failures
```

**Cause:** SSH tries multiple keys from ~/.ssh before password authentication

**Solution:** Already fixed in code with `-o PreferredAuthentications=password` option

### Tests hanging

**Cause:** Docker container taking too long to start

**Solution:** Check Docker is working: `docker ps`

### Port conflicts

**Cause:** Tests use dynamic port allocation, but collisions can occur

**Solution:** Run tests sequentially: `cargo test -- --test-threads=1`

## CI/CD Considerations

For CI environments:

```yaml
# Install dependencies
- run: sudo apt-get update && sudo apt-get install -y sshpass netcat docker.io

# Start Docker
- run: sudo systemctl start docker

# Run tests
- run: cargo test
- run: cargo test -- --ignored  # Include Docker tests
```

## Future Tests (Phase 1.4+)

Once SSH forwarding is implemented, these tests will be activated:

- [ ] Full SSH session through proxy
- [ ] Multiple concurrent SSH connections
- [ ] SSH connection timeout handling
- [ ] Large file transfer through proxy
- [ ] SSH session interruption/reconnection
- [ ] Performance benchmarks

## Test Coverage

Current coverage:
- ✅ Certificate generation (5 unit tests)
- ✅ SNI extraction (4 integration tests)
- ✅ SSH container infrastructure (2 integration tests)
- ⏸️ SSH forwarding (pending Phase 1.4)

Target coverage for Phase 1.4:
- Full end-to-end SSH proxying
- Error handling
- Connection management
- Performance characteristics
