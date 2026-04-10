use std::collections::HashMap;
use tokio::sync::{Mutex, MutexGuard, OnceCell};

static ENV_LOCK: OnceCell<Mutex<()>> = OnceCell::const_new();
/// Any test which relies on environment variables should acquire this lock and hold it for as long as it wants exclusive r/w access to the env.
pub async fn env_lock<'a>() -> MutexGuard<'a, ()> {
    ENV_LOCK
        .get_or_init(|| async { Mutex::new(()) })
        .await
        .lock()
        .await
}

/// Sets environment variables from a key-value map
pub fn set_env(vars: &HashMap<&str, &str>) {
    for (k, v) in vars {
        unsafe { std::env::set_var(k, v) };
    }
}

/// Unsets environment variables from a list of keys
pub fn unset_env(keys: &[&str]) {
    for k in keys {
        unsafe { std::env::remove_var(k) };
    }
}

/// Returns a list of sensible defaults; should be roughly equal to .env.example
pub fn default_env_vars() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("DATA_DIR", "/tmp/chelsea_test"),
        ("DB_SUBDIR", "db"),
        ("DB_NAME", "chelsea.db"),
        ("MONITORING_LOG_SUBDIR", "monitor_logs"),
        ("PROCESS_LOG_SUBDIR", "process_logs"),
        ("TIMING_LOG_OUTPUT_TARGET", "timing_logs"),
        ("KERNEL_SUBDIR", "kernels"),
        ("COMMIT_SUBDIR", "commits"),
        ("MEM_SIZE_MIB_MARGIN", "1024"),
        ("VCPU_CORES_MARGIN", "1"),
        ("MEM_SIZE_MIB_VM_MAX", "8192"),
        ("VCPU_COUNT_VM_MAX", "4"),
        ("FS_SIZE_MIB_VM_MAX", "16384"),
        ("VM_DEFAULT_IMAGE_NAME", "default"),
        ("VM_DEFAULT_KERNEL_NAME", "default.bin"),
        ("VM_DEFAULT_VCPU_COUNT", "1"),
        ("VM_DEFAULT_MEM_SIZE_MIB", "512"),
        ("VM_DEFAULT_FS_SIZE_MIB", "1024"),
        ("NETWORK_INTERFACE", "eth0"),
        ("VM_SUBNET", "192.168.100.0/24"),
        ("VM_SSH_PORT_START", "28000"),
        ("VM_SSH_PORT_END", "28128"),
        ("NETWORK_RESERVE_TIMEOUT_SECS", "10"),
        // Not sure if this should really be here.
        ("FIRECRACKER_BIN_PATH", "/usr/local/bin/firecracker"),
        ("FIRECRACKER_API_TIMEOUT_SECS", "30"),
        ("CEPH_BASE_IMAGE_SNAP_NAME", "chelsea_base_image"),
        ("CHELSEA_SERVER_PORT", "8090"),
        ("VM_ROOT_DRIVE_PATH", "dev/vda1"),
        ("AWS_COMMIT_BUCKET_NAME", "commits-bucket"),
        ("AWS_ACCESS_KEY_ID", "test"),
        ("AWS_SECRET_ACCESS_KEY", "test"),
        ("AWS_REGION", "us-east-1"),
        ("VM_DEFAULT_IMAGE_MINIMUM_SIZE_MIB", "512"),
        ("CHELSEA_EVENT_SERVER_ADDR", "127.0.0.1"),
        ("CHELSEA_EVENT_SERVER_PORT", "9090"),
        ("ORCHESTRATOR_WG_PORT", "51821"),
        ("CHELSEA_WG_INTERFACE_NAME", "wg0"),
        ("CHELSEA_WG_PORT", "51820"),
        // Proxy-specific variables
        (
            "DATABASE_URL",
            "postgres://postgres:test@localhost:5432/chelsea_test",
        ),
        ("PROXY_PRIVATE_KEY", "test_proxy_private_key_base64"),
        ("ORCHESTRATOR_PUBLIC_KEY", "test_orch_public_key_base64"),
        ("ORCHESTRATOR_PUBLIC_IP", "127.0.0.2"),
        ("ORCHESTRATOR_PRIVATE_IP", "192.168.1.1"),
        ("ORCHESTRATOR_PORT", "3000"),
        ("SSH_PORT", "8443"),
        ("SSH_CERT_PATH", "/tmp/test-cert.pem"),
        ("SSH_TLS_HANDSHAKE_TIMEOUT", "30"),
        ("SSH_BACKEND_CONNECT_TIMEOUT", "10"),
        ("SSH_IDLE_TIMEOUT", "300"),
    ])
}
