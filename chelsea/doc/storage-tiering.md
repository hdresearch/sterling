# Storage Tiering: Overlayfs + S3 Cold Storage

> RFC — March 2026

## Problem

Vers uses Ceph RBD for all VM disk storage. Every VM gets a CoW clone of a base image, and every commit creates a Ceph snapshot. This works well for performance but is expensive:

- **Ceph with 3x replication costs $0.24/GB-month** of raw storage consumed
- Committed snapshots accumulate and are rarely restored, but stay in hot storage
- When a golden image is updated, all existing VMs are stranded on the old base — they must be destroyed and recreated
- RBD CoW clones are permanently bound to their parent snapshot chain; there's no way to "rebase" onto a newer base image

### Cost breakdown (illustrative)

| Item | Count | Size | Monthly cost |
|---|---|---|---|
| Base images | 10 | 1 GB each | $2.40 |
| Active VM overlays | 500 | 200 MB avg delta | $24.00 |
| Committed snapshots (hot) | 2,000 | 500 MB avg | $240.00 |
| **Total** | | | **$266.40** |

Most of the cost is in committed snapshots that nobody is using. Moving those to S3 Standard ($0.023/GB-month) would reduce the snapshot cost from $240 to $23.

## Goals

1. **Rebasing**: update a VM's base image without losing user state
2. **Cost reduction**: move cold committed snapshots from Ceph ($0.24/GB-month with 3x replication) to S3 ($0.023/GB-month) or Glacier ($0.004/GB-month)
3. **Preserve instant branching**: creating a VM from a hot commit should remain sub-second
4. **Backward compatibility**: existing VMs continue to work; migration is incremental

## Architecture

### Two-drive overlayfs model

Today, Firecracker VMs boot from a single block device (`/dev/vda`) that is an RBD CoW clone. The proposal: split this into two drives.

```
┌──────────────────────────────────────────────────┐
│ Firecracker VM                                   │
│                                                  │
│   /dev/vda  ← base image (read-only)             │
│   /dev/vdb  ← overlay delta (read-write)         │
│                                                  │
│   init assembles overlayfs:                      │
│     lower = /dev/vda                             │
│     upper = /dev/vdb (upper/ + work/ dirs)       │
│     merged = / (what the user sees)              │
└──────────────────────────────────────────────────┘
```

**Base image** (`/dev/vda`): a shared, read-only RBD image. All VMs using the same golden image share the same base. Firecracker maps it read-only.

**Overlay delta** (`/dev/vdb`): a small, per-VM RBD image containing only the user's changes. This is what grows over time as the user installs packages, writes files, etc.

### How overlayfs works

Overlayfs is a union filesystem in the Linux kernel. It merges a read-only "lower" directory with a read-write "upper" directory:

- **Read**: if the file exists in upper, serve from upper. Otherwise, serve from lower.
- **Write**: copy the file from lower to upper on first write (copy-up), then modify in upper.
- **Delete**: create a "whiteout" file in upper that hides the lower file.
- **New file**: written directly to upper.

This means the overlay only stores files that the user actually changed or created. The base image is never modified.

### Base image updates (rebasing)

Because the overlay records changes *relative to whatever is underneath*, swapping the base image is trivial:

```
Before:  base-v1 (read-only) + overlay = user's view
After:   base-v2 (read-only) + overlay = user's view (updated base, same customizations)
```

The overlay's whiteout files and modified files take precedence over the new base, so user changes are preserved. Newly added files in base-v2 appear automatically unless the user explicitly deleted them.

**Operation**: update the `base_image_id` pointer in the DB. Next time the VM boots, it mounts the new base.

**Caveats**:
- If base-v2 changes a shared library that overlay binaries depend on, those binaries could break. This is the same constraint Docker has — base image updates should be ABI-compatible (security patches, not major version bumps).
- If the user modified a file that also changed in base-v2, the user's version wins (overlay takes precedence). This is usually the desired behavior.

### Storage tiers

```
                    ┌─────────────────┐
                    │   Ceph RBD      │
                    │   (hot tier)    │
                    │                 │
   active VMs ────▶│  base images    │◀──── shared, amortized
   recent commits ─▶│  overlay deltas │
                    └────────┬────────┘
                             │
                    background eviction
                    (overlay export to S3)
                             │
                    ┌────────▼────────┐
                    │   S3 Standard   │
                    │   (warm tier)   │
                    │                 │
                    │  overlay.tar.zst│◀──── committed overlays
                    └────────┬────────┘
                             │
                    S3 lifecycle policy
                             │
                    ┌────────▼────────┐
                    │  S3 Glacier     │
                    │  (cold tier)    │
                    │                 │
                    │  overlay.tar.zst│◀──── old commits (>30d)
                    └─────────────────┘
```

| Tier | Storage | Cost (effective) | Latency | Contents |
|---|---|---|---|---|
| Hot | Ceph RBD (3x repl.) | $0.24/GB-month | <1ms | Active VM overlays, base images, recent commits |
| Warm | S3 Standard | $0.023/GB-month | ~100ms | Committed overlays (evicted from Ceph) |
| Cold | S3 Glacier IR | $0.004/GB-month | ~minutes | Old committed overlays (auto-lifecycle) |

### Commit flow (new model)

```
1. User requests commit
2. Chelsea node:
   a. Pauses VM
   b. Snapshots the overlay drive (rbd snap create)  ← tiny, only user changes
   c. Records commit metadata: { base_image_id, overlay_snap_name }
   d. Resumes VM (or keeps paused if requested)
3. Background tiering job (later):
   a. Exports overlay snapshot: tar + zstd compress overlay contents
   b. Uploads to S3: s3://vers-overlays/{commit_id}/overlay.tar.zst
   c. Records S3 location in DB, sets tier = 'warm'
   d. Deletes Ceph snapshot (if no live clones depend on it)
```

### Restore flow (new model)

```
1. User requests restore from commit
2. Check commit tier:
   a. If hot (Ceph): clone overlay snapshot instantly, done
   b. If warm/cold (S3):
      i.   Download overlay archive from S3
      ii.  Create new empty overlay RBD image
      iii. Extract overlay contents into it
      iv.  Record in DB
3. Look up commit's base_image_id
4. Boot Firecracker with:
   - /dev/vda = base image (read-only)
   - /dev/vdb = overlay (read-write)
5. Init assembles overlayfs, pivots root
```

**Restore latency**: downloading a 200MB overlay from S3 at 500MB/s ≈ 0.4 seconds. For a 1GB overlay ≈ 2 seconds. This is acceptable for a "cold restore" UX, especially if the UI shows a progress indicator.

### Branch flow (new model)

```
1. User requests branch from running VM
2. Chelsea node:
   a. Snapshots the overlay drive
   b. Clones the snapshot → new overlay image  ← instant, CoW
   c. New VM boots with: same base image + cloned overlay
```

Branching remains instant because it's still a Ceph CoW clone — just of the overlay, not the full rootfs.

## Implementation plan

### Phase 1: Two-drive boot with overlayfs

**Goal**: VMs boot from two drives (base + overlay) with overlayfs, while remaining backward-compatible with single-drive VMs.

#### 1.1 Guest init changes

The base image's init system needs to assemble the overlayfs mount before starting userspace. This happens in the initramfs or early init:

```bash
#!/bin/sh
# /etc/chelsea/overlay-init.sh (runs before pivot_root)

# If only one drive exists, boot normally (backward compat)
if [ ! -b /dev/vdb ]; then
    exec /sbin/init "$@"
fi

# Mount base (vda) read-only
mount -o ro /dev/vda1 /mnt/base

# Mount overlay drive (vdb)
mount /dev/vdb1 /mnt/overlay
mkdir -p /mnt/overlay/upper /mnt/overlay/work

# Assemble overlayfs
mkdir -p /mnt/merged
mount -t overlay overlay \
    -o lowerdir=/mnt/base,upperdir=/mnt/overlay/upper,workdir=/mnt/overlay/work \
    /mnt/merged

# Pivot to merged root
pivot_root /mnt/merged /mnt/merged/.old_root
exec /sbin/init "$@"
```

#### 1.2 Volume manager changes

The `VmVolumeManager` trait and `CephVmVolumeManager` need to support two-volume VMs:

```rust
// New method on VmVolumeManager
async fn create_overlay_volume(
    &self,
    base_image_name: &str,
    overlay_size_mib: u32,
) -> Result<(Arc<dyn VmVolume>, Arc<dyn VmVolume>), CreateVmVolumeFromImageError>;
// Returns (base_volume_readonly, overlay_volume_readwrite)
```

The base volume is mapped read-only from the shared base image. The overlay volume is a fresh, small RBD image (formatted ext4, with `upper/` and `work/` directories).

#### 1.3 Firecracker config changes

`FirecrackerProcessConfig` needs to support multiple drives:

```rust
pub struct FirecrackerProcessConfig {
    pub boot_source: FirecrackerProcessBootSourceConfig,
    pub drives: Vec<FirecrackerProcessDriveConfig>,  // was: single `drive`
    // ...
}
```

For overlay VMs:
```rust
drives: vec![
    FirecrackerProcessDriveConfig {
        drive_id: "base".to_string(),
        path_on_host: base_volume.path(),
        is_root_device: true,
        is_read_only: true,  // base is read-only
    },
    FirecrackerProcessDriveConfig {
        drive_id: "overlay".to_string(),
        path_on_host: overlay_volume.path(),
        is_root_device: false,
        is_read_only: false,
    },
],
```

For legacy single-drive VMs (backward compat):
```rust
drives: vec![
    FirecrackerProcessDriveConfig {
        drive_id: "root".to_string(),
        path_on_host: volume.path(),
        is_root_device: true,
        is_read_only: false,
    },
],
```

#### 1.4 Commit metadata changes

Today, `VmVolumeCommitMetadata::Ceph` stores a single `snap_name`. For overlay VMs, it needs to store the base image reference and the overlay snapshot:

```rust
enum VmVolumeCommitMetadata {
    Ceph(CephVmVolumeCommitMetadata),        // legacy single-drive
    Overlay(OverlayVmVolumeCommitMetadata),   // new two-drive
}

struct OverlayVmVolumeCommitMetadata {
    base_image_name: String,          // e.g. "golden-v3"
    base_image_snap: String,          // e.g. "chelsea_base_image"
    overlay_snap_name: RbdSnapName,   // snapshot of the overlay drive
    tier: StorageTier,                // Hot, Warm, Cold
    s3_key: Option<String>,           // set when evicted to S3
}
```

#### 1.5 Database changes

New columns on `commits`:

```sql
ALTER TABLE commits ADD COLUMN base_image_name TEXT;
ALTER TABLE commits ADD COLUMN storage_tier TEXT NOT NULL DEFAULT 'hot';
-- 'hot' = in Ceph, 'warm' = S3 Standard, 'cold' = S3 Glacier
ALTER TABLE commits ADD COLUMN s3_overlay_key TEXT;
-- e.g. 'overlays/{commit_id}/overlay.tar.zst'
```

New table for base image versioning:

```sql
CREATE TABLE base_images (
    name         TEXT PRIMARY KEY,        -- e.g. "default", "golden-agent-v3"
    description  TEXT,
    rbd_image    TEXT NOT NULL,           -- Ceph RBD image name
    rbd_snap     TEXT NOT NULL,           -- Ceph snapshot name (for cloning)
    size_mib     INTEGER NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_default   BOOLEAN NOT NULL DEFAULT false
);
```

#### 1.6 Pre-warmed pool update

The `DefaultVolumePool` currently pre-warms full clones. For overlay VMs, it should pre-warm overlay drives (small, fast to create):

```rust
// Pre-warmed overlay: a small formatted ext4 image with upper/ and work/ dirs
async fn create_prewarmed_overlay(size_mib: u32) -> Result<PrewarmedOverlay> {
    let image_name = Uuid::new_v4().to_string();
    rbd.image_create(&image_name, size_mib).await?;
    let device = rbd.device_map(&image_name).await?;
    mkfs_ext4(&device).await?;
    // Mount, create upper/ and work/, unmount
    Ok(PrewarmedOverlay { image_name, device_path: device, size_mib })
}
```

### Phase 2: S3 cold storage tiering

**Goal**: automatically evict cold committed overlays from Ceph to S3.

#### 2.1 Tiering service

A background task in the orchestrator (or a standalone binary like `ceph-gc`):

```
every N minutes:
    for each commit where storage_tier = 'hot' AND age > eviction_threshold:
        if commit has no live Ceph clones (snap_has_children = false):
            export overlay to S3
            update commit: tier = 'warm', s3_key = ...
            delete Ceph snapshot
```

The existing `S3SnapshotStore` in `chelsea_lib` already handles S3 upload/download with LRU caching. We can reuse its patterns.

#### 2.2 Overlay export format

```
overlay.tar.zst
├── upper/          # overlayfs upper dir contents
│   ├── etc/
│   ├── home/
│   └── ...
├── work/           # overlayfs work dir (can be empty on restore)
└── metadata.json   # base_image_name, overlay_size_mib, created_at
```

Compressed with zstd for speed. A 200MB overlay compresses to ~80MB typically.

#### 2.3 Restore from S3

```rust
async fn restore_overlay_from_s3(
    s3_key: &str,
    base_image_name: &str,
) -> Result<(Arc<dyn VmVolume>, Arc<dyn VmVolume>)> {
    // 1. Download from S3 (with local caching)
    let archive = s3_store.ensure_available(s3_key).await?;

    // 2. Create new overlay RBD image
    let overlay = ThinVolume::new_mapped(Uuid::new_v4(), overlay_size_mib).await?;
    overlay.mkfs_ext4().await?;

    // 3. Mount and extract
    let mount_point = temp_mount(&overlay).await?;
    extract_tar_zst(&archive, &mount_point).await?;
    unmount(&mount_point).await?;

    // 4. Map base image read-only
    let base = map_base_image_readonly(base_image_name).await?;

    Ok((base, overlay))
}
```

#### 2.4 S3 lifecycle policy

Configure S3 bucket lifecycle to automatically transition objects:

```json
{
    "Rules": [{
        "ID": "overlay-tiering",
        "Status": "Enabled",
        "Transitions": [
            { "Days": 30, "StorageClass": "STANDARD_IA" },
            { "Days": 90, "StorageClass": "GLACIER_IR" }
        ],
        "Filter": { "Prefix": "overlays/" }
    }]
}
```

The DB tracks the logical tier (`warm` vs `cold`), but S3 handles the actual storage class transitions.

### Phase 3: Base image lifecycle

**Goal**: versioned base images with automated rebase.

#### 3.1 Base image versioning

```
base_images table:
  "default"     → rbd:default        @chelsea_base_image   (v1, deprecated)
  "default-v2"  → rbd:default-v2     @chelsea_base_image   (v2, current)
  "golden-agent" → rbd:golden-agent  @chelsea_base_image   (v1, current)
```

#### 3.2 Rebase operation

When a new base image version is published:

```
POST /api/v1/base-images/{name}/rebase
{
    "from_version": "default",
    "to_version": "default-v2"
}
```

For each affected commit:
1. Update `base_image_name` from `default` to `default-v2`
2. That's it — the overlay is unchanged

For running VMs, rebase takes effect on next boot. Optionally, the API can reboot affected VMs.

#### 3.3 Compatibility checking (optional, future)

Before rebasing, optionally validate that the overlay is compatible with the new base:

- Check that no overlay files conflict with critical base files (e.g., `/lib/ld-linux.so`)
- Run a test boot in a sandbox
- Flag commits that might have issues

This is a nice-to-have, not a blocker.

### Phase 4: Migration

**Goal**: migrate existing single-drive VMs to the overlay model.

#### 4.1 Online migration

For each running VM (can be done incrementally):

1. Pause the VM
2. Create a new overlay RBD image
3. Mount the VM's current rootfs and the base image
4. Compute the delta: files in the rootfs that differ from the base
5. Copy delta files into the overlay's `upper/` directory
6. Create whiteout entries for files deleted from the base
7. Update the VM record to use two-drive mode
8. Resume the VM with the new drive configuration

This is the most complex part. An alternative: only apply overlay mode to new VMs, and let old VMs age out naturally. Given that agent swarm VMs are short-lived, this is likely sufficient.

#### 4.2 Gradual rollout

```
Phase 4a: New VMs created with overlay mode (opt-in via flag)
Phase 4b: New VMs default to overlay mode
Phase 4c: Migration tool for long-lived VMs (if needed)
Phase 4d: Deprecate single-drive mode
```

## Cost projection

Assuming 500 active VMs and 2,000 committed snapshots:

| | Current (Ceph only) | With tiering |
|---|---|---|
| Base images (10 × 1GB) | $2.40 | $2.40 (shared, stays hot) |
| Active overlays (500 × 200MB) | $24.00 | $24.00 (stays hot) |
| Recent commits (200 × 500MB, <7d) | $24.00 | $24.00 (stays hot) |
| Warm commits (800 × 500MB, 7-30d) | $96.00 | $9.20 (S3 Standard) |
| Cold commits (1000 × 500MB, >30d) | $120.00 | $2.00 (S3 Glacier IR) |
| **Total** | **$266.40** | **$61.60** |
| **Savings** | | **~77%** |

Additional savings from overlay deduplication: overlays are typically much smaller than full clones because they only contain user-modified files, not rewritten base blocks.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Overlayfs edge cases (hardlinks, xattrs, whiteouts) | Subtle filesystem bugs | Extensive testing; overlayfs is mature (Docker relies on it) |
| Base image ABI breaks | User binaries crash after rebase | Only auto-rebase for security patches; manual rebase for major updates |
| S3 restore latency | Cold restores take seconds | Show progress UI; keep recent commits hot; pre-warm on demand |
| Guest kernel must support overlayfs | Older kernels might not | All Vers kernels are controlled by us; ensure CONFIG_OVERLAY_FS=y |
| Firecracker multiple drive support | Config complexity | Firecracker supports up to 16 drives; well-tested feature |
| ceph-gc changes | GC must understand overlay vs legacy | Add volume type field; GC skips overlay base images |
| Data loss during migration | Botched migration corrupts VM | Migration is optional; new VMs only; old VMs age out |

## Open questions

1. **Overlay drive default size**: how large should the initial overlay drive be? Start at 512 MiB and grow on demand? Or match the base image size?
2. **Memory snapshots**: Firecracker memory snapshots (for pause/resume) are separate from disk. Do they need any changes for overlay mode? (Probably not — the memory snapshot captures the merged view.)
3. **`resize2fs` on overlay**: when a user requests a disk resize, which drive grows — the overlay? Both? Likely just the overlay, since the base is read-only.
4. **Overlay compaction**: over time, an overlay accumulates dead files (copy-up of files that were read-then-rewritten). Should there be a compaction step that removes overlay entries identical to the base?
5. **Bandwidth metering**: the overlay export/import to S3 counts as bandwidth. Should this be billed to the user or absorbed as platform cost?
6. **Eviction threshold**: how long should a committed overlay stay hot in Ceph before eviction? 7 days? 30 days? Configurable per tier?

## Appendix: Current architecture reference

### Ceph RBD operations used

| Operation | CLI equivalent | Where used |
|---|---|---|
| `image_create` | `rbd create pool/image --size N` | New root VMs |
| `snap_create` | `rbd snap create pool/image@snap` | Commit, branch |
| `snap_protect` | `rbd snap protect pool/image@snap` | Before clone |
| `snap_clone` | `rbd clone pool/image@snap pool/new` | Branch, restore |
| `device_map` | `rbd map pool/image` | VM boot |
| `device_unmap` | `rbd device unmap /dev/rbdN` | VM stop/sleep |
| `image_grow` | `rbd resize pool/image --size N` | Disk resize |
| `snap_has_children` | `rbd children pool/image@snap` | GC safety check |

### Key files

| File | Purpose |
|---|---|
| `crates/ceph/src/client.rs` | RBD CLI wrapper |
| `crates/ceph/src/volume.rs` | `ThinVolume` — single RBD image lifecycle |
| `crates/chelsea_lib/src/volume_manager/ceph/manager.rs` | `CephVmVolumeManager` — create/commit/restore volumes |
| `crates/chelsea_lib/src/volume_manager/ceph/pool.rs` | Pre-warmed volume pool |
| `crates/chelsea_lib/src/volume_manager/manager.rs` | `VmVolumeManager` trait |
| `crates/chelsea_lib/src/volume_manager/commit.rs` | `VmVolumeCommitMetadata` enum |
| `crates/chelsea_lib/src/process/firecracker/config.rs` | Firecracker drive config |
| `crates/chelsea_lib/src/s3_store/store.rs` | S3 snapshot store with LRU cache |
| `crates/ceph-gc/src/main.rs` | Garbage collection for orphaned images/snaps |
| `crates/orchestrator/src/action/vms/commit_vm.rs` | Commit orchestration |
