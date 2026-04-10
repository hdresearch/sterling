-- migrate:up

-- A VM's node_id is NULL while it is sleeping (has a chelsea.sleep_snapshot row).
-- One of the two must always be true: node_id IS NOT NULL, or a sleep_snapshot exists.
ALTER TABLE public.vms ALTER COLUMN node_id DROP NOT NULL;

-- migrate:down

ALTER TABLE public.vms ALTER COLUMN node_id SET NOT NULL;
