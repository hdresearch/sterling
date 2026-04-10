-- migrate:up

-- Base images table: tracks available base images with ownership
-- Base images are stored in Ceph RBD, but we need to track ownership and metadata here
CREATE TABLE base_images (
    base_image_id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- User-facing image name (e.g., "ubuntu-24.04", "my-custom-image")
    -- Unique per owner, not globally unique
    image_name      TEXT NOT NULL,
    -- Internal RBD image name in Ceph - hash of (owner_id + image_name)
    -- This is globally unique and used for actual Ceph operations
    rbd_image_name  TEXT NOT NULL UNIQUE,
    -- Owner (api_key) - determines who can see/use this image
    owner_id        UUID NOT NULL REFERENCES api_keys(api_key_id) ON DELETE NO ACTION,
    -- Whether this image is visible to all accounts (system images like "default")
    is_public       BOOLEAN NOT NULL DEFAULT FALSE,
    -- Source information
    source_type     TEXT NOT NULL CHECK (source_type IN ('docker', 's3', 'manual', 'upload')),
    source_config   JSONB NOT NULL DEFAULT '{}',
    -- Image metadata
    size_mib        INTEGER NOT NULL DEFAULT 512,
    description     TEXT,
    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Each owner can only have one image with a given name
    UNIQUE (owner_id, image_name)
);

-- Index for fast lookups by owner
CREATE INDEX idx_base_images_owner_id ON base_images(owner_id);
-- Index for public images
CREATE INDEX idx_base_images_is_public ON base_images(is_public) WHERE is_public = TRUE;
-- Index for RBD image name lookups
CREATE INDEX idx_base_images_rbd_name ON base_images(rbd_image_name);

-- Base image creation jobs: tracks async image creation progress
CREATE TABLE base_image_jobs (
    job_id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- User-facing target image name
    image_name      TEXT NOT NULL,
    -- Internal RBD image name (hash of owner_id + image_name)
    rbd_image_name  TEXT NOT NULL,
    -- Owner (api_key)
    owner_id        UUID NOT NULL REFERENCES api_keys(api_key_id) ON DELETE NO ACTION,
    -- Source configuration
    source_type     TEXT NOT NULL CHECK (source_type IN ('docker', 's3', 'upload')),
    source_config   JSONB NOT NULL,
    size_mib        INTEGER NOT NULL DEFAULT 512,
    -- Job status (matches ImageCreationStatus enum from chelsea_lib)
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'downloading', 'extracting', 'configuring', 'creating_rbd', 'creating_snapshot', 'creating', 'completed', 'failed')),
    error_message   TEXT,
    -- Which node is processing this job
    node_id         UUID REFERENCES nodes(node_id) ON DELETE SET NULL,
    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ
);

-- Index for finding jobs by owner
CREATE INDEX idx_base_image_jobs_owner_id ON base_image_jobs(owner_id);
-- Index for finding pending/in-progress jobs
CREATE INDEX idx_base_image_jobs_status ON base_image_jobs(status) WHERE status NOT IN ('completed', 'failed');

-- Function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_base_image_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Triggers for updated_at
CREATE TRIGGER base_images_updated_at
    BEFORE UPDATE ON base_images
    FOR EACH ROW
    EXECUTE FUNCTION update_base_image_updated_at();

CREATE TRIGGER base_image_jobs_updated_at
    BEFORE UPDATE ON base_image_jobs
    FOR EACH ROW
    EXECUTE FUNCTION update_base_image_updated_at();

-- migrate:down

DROP TRIGGER IF EXISTS base_image_jobs_updated_at ON base_image_jobs;
DROP TRIGGER IF EXISTS base_images_updated_at ON base_images;
DROP FUNCTION IF EXISTS update_base_image_updated_at();
DROP TABLE IF EXISTS base_image_jobs;
DROP TABLE IF EXISTS base_images;
