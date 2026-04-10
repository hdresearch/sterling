-- migrate:up
ALTER TABLE vms
    ADD COLUMN deleted_at TIMESTAMPTZ;

-- migrate:down
ALTER TABLE vms
    DROP COLUMN IF EXISTS deleted_at;
