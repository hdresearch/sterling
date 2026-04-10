-- migrate:up
CREATE SCHEMA IF NOT EXISTS chelsea;

CREATE TABLE IF NOT EXISTS chelsea.commit (
    id UUID PRIMARY KEY,
    host_architecture TEXT NOT NULL,
    -- VmConfig fields
    kernel_name TEXT NOT NULL,
    base_image TEXT NOT NULL,
    vcpu_count OID NOT NULL,
    mem_size_mib OID NOT NULL,
    fs_size_mib OID NOT NULL,
    ssh_public_key TEXT NOT NULL,
    ssh_private_key TEXT NOT NULL,
    -- Discriminated union containing VolumeManager-specific data
    process_commit JSONB NOT NULL,
    volume_commit JSONB NOT NULL,
    -- List of CommitFile objects: [{"key": "s3-key"}, ...]
    remote_files JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS chelsea.vm (
    id UUID PRIMARY KEY,
    -- Discriminated union containing VolumeManager-specific data
    volume JSONB NOT NULL
);

-- VM Start Events Table
-- Tracks when VMs are started with their resource specifications
-- Used for calculating usage metrics and billing
CREATE TABLE IF NOT EXISTS chelsea.vm_start (
    vm_id UUID NOT NULL,          -- VM id
    timestamp BIGINT NOT NULL,    -- Unix timestamp when VM started
    created_at BIGINT NOT NULL,   -- Unix timestamp when this record was created
    vcpu_count OID NOT NULL,      -- Number of virtual CPUs allocated
    ram_mib OID NOT NULL,         -- RAM allocated in MiB
    disk_gib OID,                 -- Disk size in GiB (optional not logged for now)
    start_code TEXT,              -- Optional code/reason for start (not logged for now)
    -- Composite primary key prevents duplicate events for same VM at same time
    -- Allows multiple start/stop cycles for the same VM at different times
    PRIMARY KEY (vm_id, timestamp)
);

-- VM Stop Events Table  
-- Tracks when VMs are stopped with the reason for stopping
-- Used to calculate usage duration and billing periods
CREATE TABLE IF NOT EXISTS chelsea.vm_stop (
    vm_id UUID NOT NULL,          -- VM id
    timestamp BIGINT NOT NULL,    -- Unix timestamp when VM stopped
    created_at BIGINT NOT NULL,   -- Unix timestamp when this record was created
    stop_code TEXT,               -- Reason for stopping (Ex: "cluster_deletion")
    -- Composite primary key prevents duplicate events for same VM at same time
    -- Allows multiple start/stop cycles for the same VM at different times  
    PRIMARY KEY (vm_id, timestamp)
);

-- Indices for vm_start and vm_stop tables
CREATE INDEX IF NOT EXISTS idx_vm_start_vm_timestamp_desc ON chelsea.vm_start(vm_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_vm_start_timestamp ON chelsea.vm_start(timestamp);
CREATE INDEX IF NOT EXISTS idx_vm_stop_vm_timestamp_desc ON chelsea.vm_stop(vm_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_vm_stop_timestamp ON chelsea.vm_stop(timestamp);

-- ********************************************************************************************
-- ** DATA MIGRATION: Copy data from public.commits to chelsea.commit **
-- ********************************************************************************************

-- Migrate commit data to chelsea schema
INSERT INTO chelsea.commit (
    id,
    host_architecture,
    kernel_name,
    base_image,
    vcpu_count,
    mem_size_mib,
    fs_size_mib,
    ssh_public_key,
    ssh_private_key,
    process_commit,
    volume_commit,
    remote_files
)
SELECT 
    commit_id,
    host_architecture,
    kernel_name,
    base_image,
    vcpu_count,
    mem_size_mib,
    fs_size_mib,
    ssh_public_key,
    ssh_private_key,
    process_metadata,  -- maps to process_commit
    volume_metadata,   -- maps to volume_commit
    remote_files       -- already JSONB, copy as-is
FROM public.commits
ON CONFLICT DO NOTHING;

-- Create placeholder rows in chelsea.vm for existing VMs
-- The volume JSONB will be populated later from sqlite
INSERT INTO chelsea.vm (id, volume)
SELECT 
    vm_id,
    '{}'::JSONB  -- Empty JSON object as placeholder
FROM public.vms
WHERE deleted_at IS NULL
ON CONFLICT DO NOTHING;

-- ********************************************************************************************
-- ** DROP OLD COLUMNS from public.commits **
-- ********************************************************************************************

ALTER TABLE public.commits
    DROP COLUMN IF EXISTS host_architecture,
    DROP COLUMN IF EXISTS process_metadata,
    DROP COLUMN IF EXISTS volume_metadata,
    DROP COLUMN IF EXISTS kernel_name,
    DROP COLUMN IF EXISTS base_image,
    DROP COLUMN IF EXISTS vcpu_count,
    DROP COLUMN IF EXISTS mem_size_mib,
    DROP COLUMN IF EXISTS fs_size_mib,
    DROP COLUMN IF EXISTS ssh_public_key,
    DROP COLUMN IF EXISTS ssh_private_key,
    DROP COLUMN IF EXISTS remote_files;

-- migrate:down

-- Restore columns to public.commits (with nullable constraints since we can't guarantee data restoration)
ALTER TABLE public.commits
    ADD COLUMN IF NOT EXISTS host_architecture TEXT,
    ADD COLUMN IF NOT EXISTS process_metadata JSONB,
    ADD COLUMN IF NOT EXISTS volume_metadata JSONB,
    ADD COLUMN IF NOT EXISTS kernel_name TEXT,
    ADD COLUMN IF NOT EXISTS base_image TEXT,
    ADD COLUMN IF NOT EXISTS vcpu_count OID,
    ADD COLUMN IF NOT EXISTS mem_size_mib OID,
    ADD COLUMN IF NOT EXISTS fs_size_mib OID,
    ADD COLUMN IF NOT EXISTS ssh_public_key TEXT,
    ADD COLUMN IF NOT EXISTS ssh_private_key TEXT,
    ADD COLUMN IF NOT EXISTS remote_files JSONB;

-- Restore data from chelsea back to public
UPDATE public.commits c
SET 
    host_architecture = cc.host_architecture,
    process_metadata = cc.process_commit,
    volume_metadata = cc.volume_commit,
    kernel_name = cc.kernel_name,
    base_image = cc.base_image,
    vcpu_count = cc.vcpu_count,
    mem_size_mib = cc.mem_size_mib,
    fs_size_mib = cc.fs_size_mib,
    ssh_public_key = cc.ssh_public_key,
    ssh_private_key = cc.ssh_private_key,
    remote_files = cc.remote_files
FROM chelsea.commit cc
WHERE c.commit_id = cc.id;

-- Drop chelsea schema and all its objects
DROP SCHEMA IF EXISTS chelsea CASCADE;