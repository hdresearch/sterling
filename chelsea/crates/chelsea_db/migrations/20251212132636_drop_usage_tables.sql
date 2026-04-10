DROP TABLE IF EXISTS vm_start;
DROP TABLE IF EXISTS vm_stop;
DROP INDEX IF EXISTS idx_vm_start_vm_timestamp_desc;
DROP INDEX IF EXISTS idx_vm_stop_vm_timestamp_desc;
DROP INDEX IF EXISTS idx_vm_start_timestamp;
DROP INDEX IF EXISTS idx_vm_stop_timestamp;
DROP TABLE IF EXISTS usage_reporting_state;
