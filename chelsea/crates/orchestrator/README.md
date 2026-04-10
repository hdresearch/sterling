# Orchestrator

The Orchestrator is the **control plane** for Chelsea's VM management system. It coordinates VM lifecycle operations across distributed compute nodes, manages resource allocation, and exposes the public HTTP API.

## Quick Reference

| What | Where |
|------|-------|
| HTTP API handlers | `src/inbound/` |
| Business logic (actions) | `src/action/` |
| Database queries | `src/db/` |
| Chelsea node communication | `src/outbound/` |
| Background tasks | `src/bg/` |
| Configuration | `src/config/` |
| Entry point | `src/main.rs` |

## Architecture Overview

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   HTTP Client   │────▶│   Orchestrator   │────▶│  Chelsea Nodes  │
│  (public-api)   │     │   (this crate)   │     │   (VM runtime)  │
└─────────────────┘     └────────┬─────────┘     └─────────────────┘
                                 │
                        ┌────────▼─────────┐
                        │    PostgreSQL    │
                        │  (central state) │
                        └──────────────────┘
```

**Request flow:**
1. Client calls Orchestrator HTTP API (e.g., `POST /api/v1/vm/new_root`)
2. Orchestrator validates API key, selects a node, allocates resources
3. Orchestrator calls Chelsea node over WireGuard mesh
4. Chelsea node executes VM operation
5. Orchestrator persists state to PostgreSQL, returns response

## Key Concepts

### Actions (`src/action/`)

All business logic is implemented as **Actions**. An Action is an async operation with:
- Defined timeout (default: 60s)
- Access to shared context (DB, WireGuard, node communication)
- Graceful shutdown support

```rust
pub trait Action {
    type Response;
    type Error: Debug;
    const ACTION_ID: &'static str;    // For logging
    const TIMEOUT: Duration;          // Max execution time

    fn call(self, ctx: &ActionContext) -> impl Future<Output = Result<Self::Response, Self::Error>>;
}
```

**To add a new action:**
1. Create a struct in the appropriate `src/action/` submodule
2. Implement the `Action` trait
3. Call via `action::exec(YourAction { ... }).await`

### Database Layer (`src/db/`)

Repository pattern with connection pooling. Each entity type has its own repository module.

| Repository | Entity | Key Operations |
|------------|--------|----------------|
| `vms.rs` | VmEntity | create, delete, find by id/owner |
| `vm_commits.rs` | VmCommitEntity | insert, find by id |
| `nodes.rs` | NodeEntity | find all, find by orchestrator |
| `base_images.rs` | BaseImageEntity | create, update status, list |
| `keys.rs` | ApiKeyEntity | validate, find by id |

**Database access pattern:**
```rust
let db = ctx.db();
let vm = db.vm().find_by_id(vm_id).await?;
```

### HTTP Layer (`src/inbound/`)

Built with Axum. Routes defined in `mod.rs`, handlers in submodules.

**Middleware stack:**
- TraceLayer (request logging)
- TimeoutLayer (30s handler timeout)
- CompressionLayer (gzip)
- CorsLayer (permissive)

**Authentication:**
All public endpoints require Bearer token. The `AuthApiKey` extractor validates tokens via the `ValidateApiKey` action.

### Node Communication (`src/outbound/`)

The `ChelseaProto` struct handles HTTP calls to Chelsea nodes over the WireGuard mesh.

```rust
let proto = ChelseaProto::new(&node);
let status = proto.vm_status(vm_id).await?;
```

**Important:** WireGuard peer must be configured before calling a node. The proto handles this automatically.

## API Endpoints

### VM Operations

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/vm/new_root` | Create new VM from base image |
| `POST` | `/api/v1/vm/from_commit` | Restore VM from snapshot |
| `POST` | `/api/v1/vm/branch/by_commit/{id}` | Create N VMs from snapshot |
| `POST` | `/api/v1/vm/branch/by_vm/{id}` | Commit then branch running VM |
| `POST` | `/api/v1/vm/{id}/commit` | Snapshot running VM |
| `PATCH` | `/api/v1/vm/{id}/state` | Pause/Resume VM |
| `DELETE` | `/api/v1/vm/{id}` | Delete VM |
| `GET` | `/api/v1/vm/{id}` | Get VM status |
| `GET` | `/api/v1/vm/{id}/ssh_key` | Get SSH public key |
| `GET` | `/api/v1/vms` | List all VMs for user |

### Image Operations

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/v1/images` | List available images |
| `POST` | `/api/v1/images/create` | Create image from Docker/S3 |
| `POST` | `/api/v1/images/upload` | Upload tarball as image |
| `GET` | `/api/v1/images/{name}/status` | Poll creation status |

### Internal (Node Management)

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/v1/nodes/add` | Register compute node |
| `POST` | `/api/v1/nodes/{id}/remove` | Deregister compute node |

## Data Models

### VmEntity
```rust
struct VmEntity {
    vm_id: Uuid,
    parent_commit_id: Option<Uuid>,   // Snapshot this VM was created from
    grandparent_vm_id: Option<Uuid>,  // Optimization for ancestry queries
    node_id: Uuid,                    // Which Chelsea node runs this
    ip: Ipv6Addr,                     // Allocated IPv6
    wg_port: u16,                     // WireGuard port (unique per node)
    owner_id: Uuid,                   // API key that created this
    // ...
}
```

### VmCommitEntity
```rust
struct VmCommitEntity {
    id: Uuid,                         // commit_id
    parent_vm_id: Option<Uuid>,       // VM this was committed from
    grandparent_commit_id: Option<Uuid>,
    owner_id: Uuid,
    name: String,
    process_metadata: VmProcessCommitMetadata,
    volume_metadata: VmVolumeCommitMetadata,
    vm_config: VmConfigCommit,
    // ...
}
```

### NodeEntity
```rust
struct NodeEntity {
    id: Uuid,
    under_orchestrator_id: Uuid,      // Which orchestrator manages this
    ip: Option<IpAddr>,               // Public IP
    wg_ipv6: Ipv6Addr,                // Private WireGuard IP
    // ...
}
```

## Background Tasks (`src/bg/`)

Two background loops run continuously:

1. **Health Check** (5s interval): Pings each node's `/health` endpoint, tracks status transitions
2. **Image Poll** (10s interval): Checks status of pending image creation jobs

## Configuration

Required environment variables:

| Variable | Description |
|----------|-------------|
| `DATABASE_URL` | PostgreSQL connection string |
| `ORCHESTRATOR_PORT` | HTTP server port (default: 8110) |
| `ORCHESTRATOR_PUBLIC_IP` | Public IP for WireGuard |
| `ORCHESTRATOR_WG_PORT` | WireGuard port (default: 51820) |
| `PROXY_WG_PUBLIC_KEY` | Proxy's WireGuard public key |
| `PROXY_PRIVATE_IP` | Proxy's IPv6 in mesh |
| `PROXY_WG_PORT` | Proxy's WireGuard port |
| `PROXY_PUBLIC_IP` | Proxy's public IP |
| `CHELSEA_WG_PORT` | Chelsea nodes' WireGuard port |
| `CHELSEA_SERVER_PORT` | Chelsea HTTP port (default: 8111) |

## Common Tasks

### Adding a New API Endpoint

1. Define request/response types in `src/inbound/types/`
2. Create action in `src/action/` (if new business logic needed)
3. Add handler in `src/inbound/` submodule
4. Register route in `src/inbound/mod.rs`

### Adding a New Action

1. Create file in appropriate `src/action/` submodule (e.g., `src/action/vms/my_action.rs`)
2. Define struct with required fields
3. Implement `Action` trait
4. Export from `mod.rs`

Example:
```rust
pub struct MyAction {
    pub vm_id: Uuid,
}

impl Action for MyAction {
    type Response = MyResponse;
    type Error = MyError;
    const ACTION_ID: &'static str = "my_action";
    const TIMEOUT: Duration = Duration::from_secs(30);

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        let db = ctx.db();
        // ... implementation
    }
}
```

### Adding a Database Query

1. Add method to appropriate repository in `src/db/`
2. Write SQL query using `tokio_postgres`
3. Map rows to entity structs

### Debugging

- All actions log with `ACTION_ID` prefix
- HTTP requests logged via TraceLayer
- Check node health: `GET /health` on each Chelsea node
- Database state: Query PostgreSQL directly

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, server setup |
| `src/action/mod.rs` | Action framework, `exec()` function |
| `src/action/context.rs` | ActionContext definition |
| `src/inbound/mod.rs` | Route definitions |
| `src/inbound/extractors.rs` | Auth extractors |
| `src/db/mod.rs` | Database connection setup |
| `src/outbound/chelsea_proto.rs` | Chelsea node HTTP client |
| `src/bg/mod.rs` | Background task spawning |
| `src/config/mod.rs` | Environment config loading |

## Gotchas

1. **WireGuard ports are per-node unique**: The `(node_id, wg_port)` pair must be unique. Collisions trigger retry logic.

2. **Graceful shutdown**: Actions use a counter to track in-flight operations. Shutdown waits for all actions to complete.

3. **Node selection is random**: Currently no load balancing. `ChooseNode` action just picks randomly.

4. **IPv6 allocation is sequential**: VMs get the next available IP in the account's /64 subnet.

5. **API key format**: 100 chars total = 36 char UUID + 64 char hex hash. Validated with PBKDF2.

6. **Grandparent optimization**: `grandparent_vm_id` and `grandparent_commit_id` fields enable efficient ancestry traversal without multiple JOINs.
