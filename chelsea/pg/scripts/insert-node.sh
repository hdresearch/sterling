: "${POSTGRES_PASSWORD:=opensesame}"
: "${POSTGRES_USER:=postgres}"
: "${POSTGRES_DB:=vers}"

if [ -z "$1" ]; then
  echo "This script is intended to insert a single entry into the nodes table; it's intended to be run after the database has been seeded, so be sure to run ./setup-dev-db.sh first."
  echo "Usage: $0 <instance_id>"
  exit 1
fi

instance_id="$1"

psql -v instance_id="$instance_id" postgresql://$POSTGRES_USER:$POSTGRES_PASSWORD@localhost:5432/$POSTGRES_DB << EOF
-- Insert this nodes
INSERT INTO nodes (
  ip, instance_id, under_orchestrator_id, wg_ipv6, wg_public_key,
  wg_private_key, cpu_cores_total, memory_mib_total, disk_size_mib_total,
  network_count_total
) VALUES (
  '127.0.0.1', -- Chelsea IP
  :'instance_id', -- AWS Instance ID for this Chelsea node
  '18e1ecdb-6e6c-4336-868b-29f42f25ea54', -- Orchestrator ID set in _seed_orchestrator.sql
  'fd00:fe11:deed:0::1', -- Cheslea IPv6
  '59ul25nwOI5ypR5npkjcjt0ZXTWQsdq4lcf+sMkpeXg=', -- Chelsea public key
  'kPJBnRVXkFe3QDc86S1PjU8eA8xIqPM45RWGQbx+VXs=', -- Chelsea private key
  96, -- CPU cores total
  193025, -- memory MiB
  450000, -- Disk MiB
  128  -- Network count total
);
EOF

echo 'Successfully added mock node to 'nodes' table in postgres'
