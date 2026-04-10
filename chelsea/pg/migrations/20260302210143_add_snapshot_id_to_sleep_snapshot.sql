-- migrate:up

-- Add id as PK (populating existing rows with random UUIDs), soft-delete column, created_at column
ALTER TABLE chelsea.sleep_snapshot
    ADD COLUMN id uuid NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

ALTER TABLE chelsea.sleep_snapshot
    ALTER COLUMN id DROP DEFAULT;

-- Swap primary key: drop vm_id PK, add id PK
ALTER TABLE chelsea.sleep_snapshot
    DROP CONSTRAINT sleep_snapshot_pkey,
    ADD CONSTRAINT sleep_snapshot_pkey PRIMARY KEY (id);

-- migrate:down
ALTER TABLE chelsea.sleep_snapshot
    DROP CONSTRAINT IF EXISTS sleep_snapshot_pkey,
    ADD CONSTRAINT sleep_snapshot_pkey PRIMARY KEY (vm_id),
    DROP COLUMN IF EXISTS created_at,
    DROP COLUMN IF EXISTS deleted_at,
    DROP COLUMN IF EXISTS id;
