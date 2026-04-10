# Orchestrator Integration Tests

This directory contains integration tests for the orchestrator service.

## Chelsea Node Proto Tests

The `integration/node_proto.rs` tests verify the communication between the orchestrator and Chelsea nodes, particularly the VM provisioning functionality.

### Prerequisites

1. A running Chelsea server instance (either locally or on a test server)
2. The Chelsea server must be accessible from where you're running the tests

### Environment Variables

- **`CHELSEA_TEST_ENDPOINT`** (required): The IP address of the Chelsea server to test against
  - Example: `127.0.0.1` for local testing
  - Example: `10.0.1.100` for a remote test instance
  - Also accepts URL format: `http://127.0.0.1:8111` (IP will be extracted)

- **`CHELSEA_SERVER_PORT`** (optional): The port where the Chelsea server is listening
  - Default: `8111`
  - Example: `8111` for standard Chelsea server
  - This affects all node protocol communication

### Running the Tests

#### Run all node_proto integration tests:
```bash
CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 cargo test --package orchestrator --test mod integration::node_proto
```

Or with URL format:
```bash
CHELSEA_TEST_ENDPOINT=http://0.0.0.0:8111 CHELSEA_SERVER_PORT=8111 cargo test --package orchestrator --test mod integration::node_proto
```

#### Run a specific test:
```bash
CHELSEA_TEST_ENDPOINT=127.0.0.1 CHELSEA_SERVER_PORT=8111 cargo test --package orchestrator test_new_root_vm_success
```

#### List all available tests:
```bash
cargo test --package orchestrator --test mod integration::node_proto -- --list
```

### Test Coverage

The integration tests cover:

1. **`test_new_root_vm_success`**: Basic VM creation with standard configuration
2. **`test_new_root_vm_minimal_config`**: VM creation with minimal/default configuration
3. **`test_new_root_vm_custom_resources`**: VM creation with custom resource allocations
4. **`test_new_root_vm_invalid_endpoint`**: Error handling for unreachable endpoints
5. **`test_new_root_vm_multiple_sequential`**: Creating multiple VMs in sequence
6. **`test_new_root_vm_response_format`**: Validating response format and UUID structure

### Test Behavior

- Tests will automatically **skip** if `CHELSEA_TEST_ENDPOINT` is not set (rather than failing)
- Tests validate that:
  - The Chelsea server responds successfully
  - Response contains a valid v4 UUID
  - Error cases are handled correctly
  - Multiple VMs can be created sequentially

### Example Output

```bash
$ CHELSEA_TEST_ENDPOINT=127.0.0.1 cargo test --package orchestrator test_new_root_vm_success

running 1 test
test integration::node_proto::test_new_root_vm_success ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out
```

### Troubleshooting

**Tests are being skipped:**
- Ensure `CHELSEA_TEST_ENDPOINT` environment variable is set
- Check that the value is a valid IP address

**Connection refused errors:**
- Verify the Chelsea server is running
- Check that the server is listening on port 8090
- Ensure firewall rules allow connections

**Timeout errors:**
- The default timeout is 5 seconds
- Check network connectivity to the Chelsea server
- Verify the server is responding to health checks

### CI/CD Integration

To integrate these tests into your CI/CD pipeline:

```yaml
# Example GitHub Actions
- name: Run Integration Tests
  env:
    CHELSEA_TEST_ENDPOINT: ${{ secrets.TEST_SERVER_IP }}
  run: cargo test --package orchestrator --test mod integration::node_proto
```

Make sure your CI environment has access to a running Chelsea test server.
