-- migrate:up
ALTER TABLE vms ADD COLUMN wg_port INTEGER NOT NULL;

ALTER TABLE vms ADD CONSTRAINT wg_port_node_id_pair_unique UNIQUE (wg_port, node_id);

-- migrate:down
ALTER TABLE vms DROP COLUMN wg_port;
