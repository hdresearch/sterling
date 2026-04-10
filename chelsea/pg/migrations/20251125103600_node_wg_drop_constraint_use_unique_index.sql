-- migrate:up
ALTER TABLE vms DROP CONSTRAINT IF EXISTS wg_port_node_id_pair_unique;

CREATE UNIQUE INDEX IF NOT EXISTS wg_port_node_id_pair_unique_active 
ON vms (wg_port, node_id) 
WHERE deleted_at IS NULL;

-- migrate:down
DROP INDEX IF EXISTS wg_port_node_id_pair_unique_active;

ALTER TABLE vms ADD CONSTRAINT wg_port_node_id_pair_unique UNIQUE (wg_port, node_id);