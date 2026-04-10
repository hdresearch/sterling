-- migrate:up
ALTER TABLE public.nodes
    DROP COLUMN instance_id,
    ALTER COLUMN ip SET NOT NULL;

-- migrate:down
ALTER TABLE public.nodes
    ADD COLUMN instance_id UUID,
    ALTER COLUMN ip DROP NOT NULL;