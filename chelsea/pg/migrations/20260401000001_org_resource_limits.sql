-- migrate:up

-- Admin ceiling: hard platform limits, only changeable by admins.
-- Defaults are for unverified users. Bumped when an org verifies.
ALTER TABLE organizations
    ADD COLUMN admin_max_vcpus       INTEGER NOT NULL DEFAULT 8,
    ADD COLUMN admin_max_memory_mib  BIGINT  NOT NULL DEFAULT 16384;

-- User-configurable limits: what VM creation checks against.
-- Users can lower these as a safety net, but cannot exceed the admin ceiling.
-- Default to the admin ceiling values.
ALTER TABLE organizations
    ADD COLUMN max_vcpus       INTEGER NOT NULL DEFAULT 8,
    ADD COLUMN max_memory_mib  BIGINT  NOT NULL DEFAULT 16384;

-- Ensure user limits never exceed admin ceiling.
ALTER TABLE organizations
    ADD CONSTRAINT check_vcpu_limit CHECK (max_vcpus <= admin_max_vcpus),
    ADD CONSTRAINT check_memory_limit CHECK (max_memory_mib <= admin_max_memory_mib);

-- migrate:down

ALTER TABLE organizations
    DROP CONSTRAINT IF EXISTS check_memory_limit,
    DROP CONSTRAINT IF EXISTS check_vcpu_limit,
    DROP COLUMN max_memory_mib,
    DROP COLUMN max_vcpus,
    DROP COLUMN admin_max_memory_mib,
    DROP COLUMN admin_max_vcpus;
