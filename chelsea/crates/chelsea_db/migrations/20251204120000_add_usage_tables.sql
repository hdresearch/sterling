CREATE TABLE IF NOT EXISTS vm_start (
    vm_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    vcpu_count INTEGER NOT NULL CHECK (vcpu_count > 0),
    ram_mib INTEGER NOT NULL CHECK (ram_mib > 0),
    disk_gib INTEGER,
    start_code TEXT,
    PRIMARY KEY (vm_id, timestamp)
);

CREATE TABLE IF NOT EXISTS vm_stop (
    vm_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    stop_code TEXT NOT NULL,
    PRIMARY KEY (vm_id, timestamp)
);

CREATE INDEX IF NOT EXISTS idx_vm_start_vm_timestamp_desc ON vm_start(vm_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_vm_stop_vm_timestamp_desc ON vm_stop(vm_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_vm_start_timestamp ON vm_start(timestamp);
CREATE INDEX IF NOT EXISTS idx_vm_stop_timestamp ON vm_stop(timestamp);

CREATE TABLE IF NOT EXISTS usage_reporting_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_reported_interval_start INTEGER NOT NULL,
    last_reported_interval_end INTEGER NOT NULL,
    last_report_time INTEGER NOT NULL
);
