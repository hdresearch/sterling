-- migrate:up

-- Add telemetry columns to node_heartbeats for resource-based node scoring
-- These values are populated during health checks when a node reports "Up"

ALTER TABLE node_heartbeats
    ADD COLUMN vcpu_available INTEGER,
    ADD COLUMN mem_mib_available BIGINT;

COMMENT ON COLUMN node_heartbeats.vcpu_available IS 'Available vCPUs on the node at time of health check';
COMMENT ON COLUMN node_heartbeats.mem_mib_available IS 'Available memory in MiB on the node at time of health check';

-- migrate:down

ALTER TABLE node_heartbeats
    DROP COLUMN IF EXISTS vcpu_available,
    DROP COLUMN IF EXISTS mem_mib_available;
