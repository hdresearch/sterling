CREATE TABLE IF NOT EXISTS vm_network (
    host_addr INTEGER PRIMARY KEY,
    vm_addr INTEGER NOT NULL,
    netns_name TEXT NOT NULL,
    ssh_port INTEGER NOT NULL,
    wg_interface_name TEXT,
    wg_private_key TEXT,
    wg_private_ip TEXT,
    wg_peer_pub_key TEXT,
    wg_peer_pub_ip TEXT,
    wg_peer_prv_ip TEXT,
    wg_port INTEGER,
    reserved_until TEXT  -- RFC 3339
);

CREATE TABLE IF NOT EXISTS vm (
    id TEXT PRIMARY KEY NOT NULL,
    ssh_public_key TEXT NOT NULL,
    ssh_private_key TEXT NOT NULL,
    parent_id TEXT,
    kernel_name TEXT NOT NULL,
    image_name TEXT NOT NULL,
    vcpu_count INTEGER NOT NULL,
    mem_size_mib INTEGER NOT NULL,
    fs_size_mib INTEGER NOT NULL,
    vm_network_host_addr INTEGER NOT NULL,
    vm_process_pid INTEGER NOT NULL,
    vm_volume_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS vm_process (
    pid INTEGER PRIMARY KEY NOT NULL,
    process_type TEXT NOT NULL,
    vm_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ceph_vm_volume (
    id TEXT PRIMARY KEY NOT NULL,
    size TEXT NOT NULL,
    image_name TEXT NOT NULL,
    device_path TEXT NOT NULL,
    current_snap TEXT
);

CREATE TABLE IF NOT EXISTS node_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE VIEW IF NOT EXISTS vm_sum AS
SELECT
    SUM(vcpu_count) AS vcpu_count_sum,
    SUM(mem_size_mib) AS mem_size_mib_sum
FROM vm;
