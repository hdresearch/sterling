-- migrate:up

ALTER TABLE commits
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN deleted_by UUID REFERENCES api_keys(api_key_id);

CREATE INDEX IF NOT EXISTS idx_commits_deleted_at ON commits(deleted_at);

ALTER TABLE chelsea.commit
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN deleted_by UUID;

-- migrate:down

ALTER TABLE commits
    DROP COLUMN IF EXISTS deleted_at,
    DROP COLUMN IF EXISTS deleted_by;

DROP INDEX IF EXISTS idx_commits_deleted_at;

ALTER TABLE chelsea.commit
    DROP COLUMN IF EXISTS deleted_at,
    DROP COLUMN IF EXISTS deleted_by;
