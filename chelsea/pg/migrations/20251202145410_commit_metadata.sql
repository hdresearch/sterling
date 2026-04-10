-- migrate:up
ALTER TABLE commits
    ADD COLUMN host_architecture TEXT NOT NULL DEFAULT 'x86_64',
    ADD COLUMN process_metadata JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN volume_metadata JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN kernel_name TEXT NOT NULL DEFAULT '',
    ADD COLUMN base_image TEXT NOT NULL DEFAULT '',
    ADD COLUMN vcpu_count OID NOT NULL DEFAULT 1,
    ADD COLUMN mem_size_mib OID NOT NULL DEFAULT 512,
    ADD COLUMN fs_size_mib OID NOT NULL DEFAULT 1024,
    ADD COLUMN ssh_public_key TEXT NOT NULL DEFAULT '',
    ADD COLUMN ssh_private_key TEXT NOT NULL DEFAULT '',
    ADD COLUMN remote_files JSONB NOT NULL DEFAULT '[]';

-- Remove defaults after adding columns (so existing rows get defaults, but new inserts must provide values)
ALTER TABLE commits
    ALTER COLUMN host_architecture DROP DEFAULT,
    ALTER COLUMN process_metadata DROP DEFAULT,
    ALTER COLUMN volume_metadata DROP DEFAULT,
    ALTER COLUMN kernel_name DROP DEFAULT,
    ALTER COLUMN base_image DROP DEFAULT,
    ALTER COLUMN vcpu_count DROP DEFAULT,
    ALTER COLUMN mem_size_mib DROP DEFAULT,
    ALTER COLUMN fs_size_mib DROP DEFAULT,
    ALTER COLUMN ssh_public_key DROP DEFAULT,
    ALTER COLUMN ssh_private_key DROP DEFAULT,
    ALTER COLUMN remote_files DROP DEFAULT;

-- migrate:down
ALTER TABLE commits
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