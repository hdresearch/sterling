-- migrate:up

CREATE TABLE IF NOT EXISTS chelsea.vm_usage_segments (
    vm_id UUID NOT NULL,
    start_timestamp BIGINT NOT NULL,
    start_created_at BIGINT NOT NULL,
    stop_timestamp BIGINT,
    stop_created_at BIGINT,
    vcpu_count OID NOT NULL,
    ram_mib OID NOT NULL,
    disk_gib OID,
    start_code TEXT,
    stop_code TEXT,
    PRIMARY KEY (vm_id, start_timestamp),
    CHECK (stop_timestamp IS NULL OR stop_timestamp >= start_timestamp),
    CHECK (
        (stop_timestamp IS NULL AND stop_created_at IS NULL)
        OR (stop_timestamp IS NOT NULL AND stop_created_at IS NOT NULL)
    )
);

INSERT INTO chelsea.vm_usage_segments (
    vm_id,
    start_timestamp,
    start_created_at,
    stop_timestamp,
    stop_created_at,
    vcpu_count,
    ram_mib,
    disk_gib,
    start_code,
    stop_code
)
SELECT
    vm_id,
    timestamp AS start_timestamp,
    created_at AS start_created_at,
    NULL::BIGINT,
    NULL::BIGINT,
    vcpu_count,
    ram_mib,
    disk_gib,
    start_code,
    NULL::TEXT
FROM chelsea.vm_start
ON CONFLICT (vm_id, start_timestamp) DO NOTHING;

WITH ranked_starts AS (
    SELECT
        vm_id,
        timestamp AS start_timestamp,
        ROW_NUMBER() OVER (PARTITION BY vm_id ORDER BY timestamp) AS rn
    FROM chelsea.vm_start
),
ranked_stops AS (
    SELECT
        vm_id,
        timestamp AS stop_timestamp,
        created_at AS stop_created_at,
        stop_code,
        ROW_NUMBER() OVER (PARTITION BY vm_id ORDER BY timestamp) AS rn
    FROM chelsea.vm_stop
)
UPDATE chelsea.vm_usage_segments AS seg
SET
    stop_timestamp = ranked_stops.stop_timestamp,
    stop_created_at = ranked_stops.stop_created_at,
    stop_code = ranked_stops.stop_code
FROM ranked_starts
INNER JOIN ranked_stops
    ON ranked_starts.vm_id = ranked_stops.vm_id
   AND ranked_starts.rn = ranked_stops.rn
WHERE seg.vm_id = ranked_starts.vm_id
  AND seg.start_timestamp = ranked_starts.start_timestamp;

DROP TABLE IF EXISTS chelsea.vm_stop;
DROP TABLE IF EXISTS chelsea.vm_start;

CREATE INDEX IF NOT EXISTS idx_vm_usage_segments_vm_start_desc
    ON chelsea.vm_usage_segments (vm_id, start_timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_vm_usage_segments_start_timestamp
    ON chelsea.vm_usage_segments (start_timestamp);

CREATE INDEX IF NOT EXISTS idx_vm_usage_segments_stop_timestamp
    ON chelsea.vm_usage_segments (stop_timestamp)
    WHERE stop_timestamp IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_vm_usage_segments_open_spans
    ON chelsea.vm_usage_segments (vm_id, start_timestamp)
    WHERE stop_timestamp IS NULL;

CREATE OR REPLACE FUNCTION chelsea.enforce_vm_usage_segment_stop_update()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.vm_id <> OLD.vm_id
        OR NEW.start_timestamp <> OLD.start_timestamp
        OR NEW.start_created_at <> OLD.start_created_at
        OR NEW.vcpu_count <> OLD.vcpu_count
        OR NEW.ram_mib <> OLD.ram_mib
        OR NEW.disk_gib IS DISTINCT FROM OLD.disk_gib
        OR NEW.start_code IS DISTINCT FROM OLD.start_code THEN
        RAISE EXCEPTION 'vm_usage_segments start fields are immutable';
    END IF;

    IF OLD.stop_timestamp IS NOT NULL THEN
        RAISE EXCEPTION 'stop metadata already recorded for this VM segment';
    END IF;

    IF NEW.stop_timestamp IS NULL THEN
        RAISE EXCEPTION 'stop_timestamp must be set when updating VM usage segments';
    END IF;

    IF NEW.stop_created_at IS NULL THEN
        RAISE EXCEPTION 'stop_created_at must be set when updating VM usage segments';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_vm_usage_segments_stop_update ON chelsea.vm_usage_segments;

CREATE TRIGGER trg_vm_usage_segments_stop_update
BEFORE UPDATE ON chelsea.vm_usage_segments
FOR EACH ROW
EXECUTE FUNCTION chelsea.enforce_vm_usage_segment_stop_update();

-- migrate:down

DROP TRIGGER IF EXISTS trg_vm_usage_segments_stop_update ON chelsea.vm_usage_segments;
DROP FUNCTION IF EXISTS chelsea.enforce_vm_usage_segment_stop_update();

CREATE TABLE IF NOT EXISTS chelsea.vm_start (
    vm_id UUID NOT NULL,
    timestamp BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    vcpu_count OID NOT NULL,
    ram_mib OID NOT NULL,
    disk_gib OID,
    start_code TEXT,
    PRIMARY KEY (vm_id, timestamp)
);

CREATE TABLE IF NOT EXISTS chelsea.vm_stop (
    vm_id UUID NOT NULL,
    timestamp BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    stop_code TEXT,
    PRIMARY KEY (vm_id, timestamp)
);

INSERT INTO chelsea.vm_start (
    vm_id,
    timestamp,
    created_at,
    vcpu_count,
    ram_mib,
    disk_gib,
    start_code
)
SELECT
    vm_id,
    start_timestamp AS timestamp,
    start_created_at AS created_at,
    vcpu_count,
    ram_mib,
    disk_gib,
    start_code
FROM chelsea.vm_usage_segments
ON CONFLICT (vm_id, timestamp) DO NOTHING;

INSERT INTO chelsea.vm_stop (
    vm_id,
    timestamp,
    created_at,
    stop_code
)
SELECT
    vm_id,
    stop_timestamp AS timestamp,
    stop_created_at AS created_at,
    stop_code
FROM chelsea.vm_usage_segments
WHERE stop_timestamp IS NOT NULL
ON CONFLICT (vm_id, timestamp) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_vm_start_vm_timestamp_desc ON chelsea.vm_start(vm_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_vm_start_timestamp ON chelsea.vm_start(timestamp);
CREATE INDEX IF NOT EXISTS idx_vm_stop_vm_timestamp_desc ON chelsea.vm_stop(vm_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_vm_stop_timestamp ON chelsea.vm_stop(timestamp);

DROP TABLE IF EXISTS chelsea.vm_usage_segments;
