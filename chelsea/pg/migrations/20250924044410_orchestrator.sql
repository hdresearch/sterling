-- migrate:up

CREATE TABLE orchestrators (
  id UUID NOT NULL PRIMARY KEY,
  region TEXT NOT NULL UNIQUE,
  wg_public_key TEXT UNIQUE NOT NULL,
  wg_private_key TEXT UNIQUE NOT NULL,
  wg_ipv6 INET UNIQUE NOT NULL,
  ip INET NOT NULL,
  created_at TIMESTAMPTZ NOT NULL

  CHECK (
    family(wg_ipv6) = 6 AND
    masklen(wg_ipv6) = 128
  )
);

ALTER TABLE nodes
  DROP CONSTRAINT nodes_status_check,
  DROP COLUMN status,
  DROP COLUMN cpu,
  DROP COLUMN mem_gb,
  DROP COLUMN disk_gb,
  DROP COLUMN provider,
  DROP COLUMN last_heartbeat;

ALTER TABLE nodes
  ADD UNIQUE (ip),
  ADD COLUMN instance_id TEXT NOT NULL UNIQUE,
  ADD COLUMN under_orchestrator_id UUID NOT NULL REFERENCES orchestrators(id),

  ADD COLUMN wg_ipv6 INET NOT NULL UNIQUE,
  ADD CONSTRAINT check_ipv6_assign
  CHECK (
    family(wg_ipv6) = 6 AND
    masklen(wg_ipv6) = 128
  ),

  ADD COLUMN wg_public_key TEXT NOT NULL,
  ADD COLUMN wg_private_key TEXT NOT NULL,

  ADD COLUMN cpu_cores_total INTEGER NOT NULL,
  ADD COLUMN memory_mib_total BIGINT NOT NULL,
  ADD COLUMN disk_size_mib_total BIGINT NOT NULL,
  ADD COLUMN network_count_total INTEGER NOT NULL;

CREATE OR REPLACE FUNCTION trg_ipv6_chelsea_node_assign()
  RETURNS trigger
  LANGUAGE plpgsql
AS $$
DECLARE
    highest_ip inet;
BEGIN
    IF NEW.wg_ipv6 IS NULL THEN
        -- This is probably very bad practice
        PERFORM 1 FROM nodes FOR UPDATE;
        
        SELECT MAX(wg_ipv6) INTO highest_ip FROM nodes;
        
        IF highest_ip IS NULL THEN
            NEW.wg_ipv6 := 'fd00:fe11:deed:0::100';
        ELSE
            NEW.wg_ipv6 := highest_ip + 1;
        END IF;
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER setup_ipv6_assigns_on_chelsea_insert
BEFORE INSERT ON nodes
FOR EACH ROW EXECUTE FUNCTION trg_ipv6_chelsea_node_assign();

CREATE TABLE node_heartbeats (
  node_id uuid NOT NULL REFERENCES nodes(node_id) ON DELETE CASCADE,
  timestamp timestamptz NOT NULL,
  status text NOT NULL,
  PRIMARY KEY (node_id, timestamp)
);

ALTER TABLE commits ADD COLUMN vm_id UUID NOT NULL REFERENCES vms(vm_id);


-- migrate:down
