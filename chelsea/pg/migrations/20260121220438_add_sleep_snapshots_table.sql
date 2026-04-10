-- migrate:up

CREATE TABLE IF NOT EXISTS chelsea.sleep_snapshot (
    vm_id uuid PRIMARY KEY,
    host_architecture TEXT NOT NULL,
    kernel_name TEXT NOT NULL,
    base_image TEXT NOT NULL,
    vcpu_count OID NOT NULL,
    mem_size_mib OID NOT NULL,
    fs_size_mib OID NOT NULL,
    ssh_public_key TEXT NOT NULL,
    ssh_private_key TEXT NOT NULL,
    -- Discriminated union containing VolumeManager-specific data
    process_sleep_snapshot JSONB NOT NULL,
    volume_sleep_snapshot JSONB NOT NULL,
    -- List of CommitFile objects: [{"key": "s3-key"}, ...]
    remote_files JSONB NOT NULL
);

-- migrate:down

DROP TABLE IF EXISTS chelsea.sleep_snapshot;
