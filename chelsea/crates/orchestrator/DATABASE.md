# Database Schema

The orchestrator uses PostgreSQL for persistent state. This document reflects the actual schema used in production.

## Tables

### `vms`

Active and deleted virtual machines.

| Column | Type | Description |
|--------|------|-------------|
| `vm_id` | UUID | Primary key |
| `parent_commit_id` | UUID | Commit this VM was restored from (nullable) |
| `grandparent_vm_id` | UUID | VM that parent commit came from (nullable, optimization) |
| `node_id` | UUID | FK to chelsea_nodes |
| `ip` | INET | Allocated IPv6 address |
| `wg_private_key` | TEXT | WireGuard private key |
| `wg_public_key` | TEXT | WireGuard public key |
| `wg_port` | INT | WireGuard port (unique per node) |
| `owner_id` | UUID | FK to api_keys |
| `created_at` | TIMESTAMPTZ | Creation time |
| `deleted_at` | TIMESTAMPTZ | Soft delete timestamp (nullable) |

**Constraints:**
- `UNIQUE (node_id, wg_port)` - Prevents port collisions on same node

### `vm_commits`

VM snapshots/checkpoints.

| Column | Type | Description |
|--------|------|-------------|
| `commit_id` | UUID | Primary key |
| `parent_vm_id` | UUID | VM this commit was created from (nullable) |
| `grandparent_commit_id` | UUID | Commit that parent VM started from (nullable) |
| `owner_id` | UUID | FK to api_keys |
| `name` | TEXT | Human-readable name |
| `description` | TEXT | Optional description |
| `created_at` | TIMESTAMPTZ | Creation time |
| `host_architecture` | TEXT | e.g., "x86_64" |
| `process_metadata` | JSONB | VM process state |
| `volume_metadata` | JSONB | Disk/volume info |
| `kernel_name` | TEXT | Kernel binary name |
| `base_image` | TEXT | Base image name |
| `remote_files` | JSONB | List of committed files |

### `chelsea_nodes`

Compute nodes running Chelsea.

| Column | Type | Description |
|--------|------|-------------|
| `id` | UUID | Primary key |
| `instance_id` | TEXT | Cloud instance ID (nullable) |
| `under_orchestrator_id` | UUID | FK to orchestrators |
| `ip` | INET | Public IP (nullable) |
| `wg_private_key` | TEXT | WireGuard private key |
| `wg_public_key` | TEXT | WireGuard public key |
| `wg_ipv6` | INET | Private IPv6 in WireGuard mesh |
| `resources` | JSONB | Node capacity info |
| `created_at` | TIMESTAMPTZ | Registration time |

### `base_images`

Managed base images for VM creation.

| Column | Type | Description |
|--------|------|-------------|
| `image_name` | TEXT | Primary key (user-facing name) |
| `rbd_image_name` | TEXT | Ceph RBD name (`{owner_id}/{image_name}`) |
| `owner_id` | UUID | FK to api_keys |
| `is_public` | BOOLEAN | Visible to all users |
| `source_type` | TEXT | "Docker", "S3", "Upload", "Manual" |
| `source_config` | JSONB | Source-specific config |
| `size_mib` | INT | Image size in MiB |
| `description` | TEXT | Optional description |
| `status` | TEXT | "Pending", "Ready", "Failed" |
| `created_at` | TIMESTAMPTZ | Creation time |

### `api_keys`

API authentication credentials.

| Column | Type | Description |
|--------|------|-------------|
| `api_key_id` | UUID | Primary key |
| `user_id` | UUID | FK to users |
| `org_id` | UUID | FK to organizations |
| `label` | TEXT | Human-readable label |
| `key_algo` | TEXT | Hash algorithm ("PBKDF2") |
| `key_iter` | INT | PBKDF2 iterations |
| `key_salt` | TEXT | Hex-encoded salt |
| `key_hash` | TEXT | Hex-encoded hash |
| `is_active` | BOOLEAN | Key can be used |
| `is_deleted` | BOOLEAN | Soft delete flag |
| `created_at` | TIMESTAMPTZ | Creation time |
| `expires_at` | TIMESTAMPTZ | Expiration (nullable) |
| `revoked_at` | TIMESTAMPTZ | Revocation time (nullable) |
| `deleted_at` | TIMESTAMPTZ | Soft delete time (nullable) |

### `accounts`

Billing accounts (own /64 IPv6 subnets).

| Column | Type | Description |
|--------|------|-------------|
| `account_id` | UUID | Primary key |
| `name` | TEXT | Account name |
| `billing_email` | TEXT | Billing contact |
| `ipv6_subnet` | INET | Account's /64 subnet for VMs |
| `created_at` | TIMESTAMPTZ | Creation time |
| `expires_at` | TIMESTAMPTZ | Account expiration |

### `organizations`

Organizations within accounts.

| Column | Type | Description |
|--------|------|-------------|
| `org_id` | UUID | Primary key |
| `account_id` | UUID | FK to accounts |
| `name` | TEXT | Organization name |
| `created_at` | TIMESTAMPTZ | Creation time |

### `orchestrators`

Orchestrator instances (one per region).

| Column | Type | Description |
|--------|------|-------------|
| `orchestrator_id` | UUID | Primary key |
| `region` | TEXT | Region identifier |
| `wg_ipv6` | INET | IPv6 in WireGuard mesh |
| `wg_private_key` | TEXT | WireGuard private key |
| `wg_public_key` | TEXT | WireGuard public key |
| `ip` | INET | Public IP |
| `created_at` | TIMESTAMPTZ | Registration time |

### `node_health_history`

Historical node health records.

| Column | Type | Description |
|--------|------|-------------|
| `id` | SERIAL | Primary key |
| `node_id` | UUID | FK to chelsea_nodes |
| `status` | TEXT | "Up", "Down", "Booting", "Unknown" |
| `recorded_at` | TIMESTAMPTZ | Check time |

## Key Relationships

```
accounts
  └── organizations
        └── api_keys
              └── vms ──────────┐
              └── vm_commits ◄──┘
              └── base_images

orchestrators
  └── chelsea_nodes
        └── vms
        └── node_health_history
```

## Database Functions

### `next_vm_ip(account_id UUID) → INET`

Returns the next available IPv6 address in the account's /64 subnet.

### `next_vm_wg_port(node_id UUID) → INT`

Returns the next available WireGuard port for the given node.

## Connection Settings

- **Pool size:** 16 connections max
- **TLS:** Enabled for non-localhost connections
- **Timeout:** Connections timeout after 60 seconds of inactivity

## Testing

For integration tests, set `TEST_DATABASE_URL`. The test harness wraps queries in transactions that roll back after each test.
