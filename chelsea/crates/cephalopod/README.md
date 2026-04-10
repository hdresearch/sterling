# Cephalopod 🐙

Native Rust bindings to **librados** and **librbd** for direct Ceph RBD access without shelling out to the `rbd` CLI.

## Why

The old `ceph` crate spawns a new `rbd` process for every operation, parses JSON/text output, and re-establishes the cluster connection each time. Cephalopod connects once and makes direct library calls — no process overhead, no output parsing, structured errors natively.

Device map/unmap operations use direct sysfs writes to the kernel `krbd` module (`/sys/bus/rbd/add_single_major` and `remove_single_major`) — no process spawns at all.

## System Requirements

You need the Ceph development libraries installed:

```bash
sudo apt install librados-dev librbd-dev
```

And a Ceph cluster accessible via `/etc/ceph/ceph.conf` with a client keyring at `/etc/ceph/ceph.client.<id>.keyring`.

## Usage

### Async Client (recommended)

The `Client` is the primary API — an async wrapper that runs FFI calls on the Tokio blocking threadpool.

```rust
use cephalopod::Client;

// Connect once, reuse for all operations
let client = Client::connect("chelsea", "rbd")?;

// Image operations
client.image_create("my-image", 1024).await?;       // 1024 MiB
client.image_grow("my-image", 2048).await?;
let info = client.image_info("my-image").await?;
println!("size: {} MiB", info.size_mib());
client.image_remove("my-image").await?;

// Snapshot operations
client.snap_create("my-image", "snap1").await?;
client.snap_protect("my-image", "snap1").await?;
client.snap_clone("my-image", "snap1", "my-clone").await?;
client.snap_unprotect("my-image", "snap1").await?;
client.snap_remove("my-image", "snap1").await?;

// Namespace operations
client.namespace_ensure("my-namespace").await?;
client.image_create("my-namespace/my-image", 512).await?;

// Cross-namespace clone
client.snap_create("my-namespace/base", "v1").await?;
client.snap_protect("my-namespace/base", "v1").await?;
client.snap_clone("my-namespace/base", "v1", "vm-instance").await?;

// Device map/unmap (native sysfs writes — requires root)
let device_path = client.device_map("my-image").await?;
client.device_unmap(&device_path).await?;   // retries on EBUSY
```

### Namespaced Image Names

Image names can include a namespace prefix separated by `/`:

- `"my-image"` → default namespace
- `"owner_id/my-image"` → namespace `owner_id`

The client parses this automatically and sets the ioctx namespace for each operation. Cross-namespace clones (e.g. namespace → default) work via clone format v2.

### Low-level Blocking API

For non-async contexts or custom usage, the `rbd` module exposes blocking functions directly:

```rust
use cephalopod::{RadosCluster, RadosIoCtx};
use cephalopod::rbd;

let cluster = RadosCluster::connect("chelsea")?;
let ioctx = cluster.ioctx("rbd")?;

rbd::image_create(&ioctx, "my-image", 1024)?;
let info = rbd::image_stat(&ioctx, "my-image")?;
rbd::image_remove(&ioctx, "my-image")?;

// Work in a namespace
ioctx.set_namespace("my-namespace")?;
rbd::image_create(&ioctx, "namespaced-image", 512)?;
```

## Error Handling

All operations return `Result<T, CephalopodError>`. Errors map Ceph errno values to structured variants:

| Variant | Meaning |
|---|---|
| `NotFound` | ENOENT — image, snap, or namespace doesn't exist |
| `AlreadyExists` | EEXIST — image, snap, or namespace already exists |
| `Ceph { errno, message, context }` | Any other librados/librbd error |
| `Device` | A `device_map`/`device_unmap` sysfs operation failure |
| `NulByte` | An argument contained an interior NUL byte |

## Testing

Requires a running Ceph cluster with the `chelsea` client and an `rbd` pool:

```bash
cargo nextest run -p cephalopod
```

50 integration tests cover connection handling, image CRUD, snapshots (create/protect/unprotect/clone/purge/children), namespaces, cross-namespace clones, device map/unmap, and error cases. Device map/unmap tests require root.
