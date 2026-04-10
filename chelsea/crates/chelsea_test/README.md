# Chelsea Test Framework

Integration test framework for Chelsea that exposes `VmManager` - the same interface the production web server uses.

## Requirements

- **Root/CAP_NET_ADMIN** - for networking (iptables, namespaces)
- **Ceph cluster** - for volume management
- **Docker** - for PostgreSQL testcontainer
- **dbmate** - for database migrations
- **psql** - for database creation

## Usage

```rust
use chelsea_test::run_test;

#[test]
fn test_vm_lifecycle() {
    run_test(|env| async move {
        let vm_manager = env.vm_manager();
        
        // Create, verify, delete VM...
        
        Ok(())
    });
}
```

## Running Tests

Add integration tests in any crate using `chelsea_test::run_test(...)`, then run them with root/Ceph access, e.g.:

```bash
# Must run as root for networking
sudo cargo test -p <your_crate> -- --nocapture
```
