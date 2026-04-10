-- migrate:up

-- Store requested resource allocations on the VM row so the orchestrator can
-- enforce per-org limits atomically (SELECT ... FOR UPDATE on the org +
-- counting from vms in a single transaction).
ALTER TABLE vms
    ADD COLUMN vcpu_count  INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN mem_size_mib INTEGER NOT NULL DEFAULT 512;

-- Backfill from the latest open usage segment for each VM (if any).
UPDATE vms
SET vcpu_count  = seg.vcpu_count,
    mem_size_mib = seg.ram_mib
FROM (
    SELECT DISTINCT ON (vm_id) vm_id, vcpu_count, ram_mib
    FROM chelsea.vm_usage_segments
    ORDER BY vm_id, start_timestamp DESC
) seg
WHERE vms.vm_id = seg.vm_id;

-- migrate:down

ALTER TABLE vms
    DROP COLUMN IF EXISTS mem_size_mib,
    DROP COLUMN IF EXISTS vcpu_count;
