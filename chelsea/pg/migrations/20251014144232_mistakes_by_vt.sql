-- migrate:up

ALTER TABLE nodes
  ALTER COLUMN ip DROP NOT NULL,
  ALTER COLUMN instance_id DROP NOT NULL;

-- migrate:down
