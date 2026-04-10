use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Range,
    path::{Path, PathBuf},
    str::FromStr,
    sync::OnceLock,
};

use ipnet::Ipv4Net;
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use crate::{config_dictionary::get_config_dictionary, error::GetSourcesError, warn};

const DEFAULT_CONFIG_DIR: &str = "/etc/vers";

static VERS_CONFIG: OnceLock<VersConfig> = OnceLock::new();
fn static_vers_config() -> &'static VersConfig {
    VERS_CONFIG.get_or_init(|| {
        VersConfig::create_from_default_config_dir().expect("Failed to initialize VersConfig")
    })
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CommonConfig {
    // Database config
    pub database_url: String,

    // Alerting config
    pub discord_alert_webhook_url: String,
    pub pagerduty_alert_routing_key: Option<String>,
}

/// A struct showing the Json representation of Range
#[derive(Debug, Serialize, ToSchema)]
pub struct RangeJson<Idx> {
    start: Idx,
    end: Idx,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HypervisorType {
    CloudHypervisor,
    #[default]
    Firecracker,
}

impl std::fmt::Display for HypervisorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Firecracker => write!(f, "firecracker"),
            Self::CloudHypervisor => write!(f, "cloud-hypervisor"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid hypervisor type: '{0}'")]
pub struct InvalidHypervisorType(String);

impl FromStr for HypervisorType {
    type Err = InvalidHypervisorType;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "firecracker" => Ok(Self::Firecracker),
            "cloud-hypervisor" => Ok(Self::CloudHypervisor),
            _ => Err(InvalidHypervisorType(s.to_string())),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChelseaConfig {
    #[schema(value_type = String)]
    pub event_server_addr: Ipv4Addr,
    pub event_server_port: u16,
    pub server_port: u16,
    // Wireguard config
    pub wg_interface_name: String,
    pub wg_port: u16,
    // Data directories
    #[schema(value_type = String)]
    pub data_dir: PathBuf,
    #[schema(value_type = String)]
    pub monitoring_log_dir: PathBuf,
    pub timing_log_target: String,
    #[schema(value_type = String)]
    pub process_log_dir: PathBuf,
    #[schema(value_type = String)]
    pub kernel_dir: PathBuf,
    #[schema(value_type = String)]
    pub snapshot_dir: PathBuf,
    #[schema(value_type = String)]
    pub db_path: PathBuf,
    // Resource limits
    pub snapshot_dir_max_size_mib: u32,
    // VM provisioning parameters
    pub vm_max_vcpu_count: u32,
    pub vm_max_memory_mib: u32,
    pub vm_max_volume_mib: u32,
    pub allow_vcpu_overprovisioning: bool,
    pub allow_memory_overprovisioning: bool,
    pub vm_total_vcpu_count: u32,
    pub vm_total_memory_mib: u32,
    // VM config defaults
    pub vm_default_image_name: String,
    pub vm_default_kernel_name: String,
    pub vm_default_vcpu_count: u32,
    pub vm_default_mem_size_mib: u32,
    pub vm_default_fs_size_mib: u32,
    // Network manager config
    #[schema(value_type = String)]
    pub vm_subnet: Ipv4Net,
    #[schema(value_type = RangeJson<u16>)]
    pub vm_ssh_port_range: Range<u16>,
    pub network_reserve_timeout_secs: u16,
    // Firecracker-specific
    #[schema(value_type = String)]
    pub firecracker_bin_path: PathBuf,
    pub firecracker_api_timeout_secs: u64,
    pub firecracker_socket_timeout_secs: u64,
    pub firecracker_use_huge_pages: bool,
    // Cloud Hypervisor-specific
    #[schema(value_type = Option<String>)]
    pub cloud_hypervisor_bin_path: Option<PathBuf>,
    // Hypervisor selection and per-hypervisor kernel names
    pub hypervisor_type: HypervisorType,
    pub firecracker_kernel_name: String,
    pub cloud_hypervisor_kernel_name: String,
    // Cgroup isolation for VM processes
    pub vm_cgroup_name: String,
    pub vm_cgroup_cpu_weight: u32,
    // Ceph
    pub ceph_base_image_snap_name: String,
    pub ceph_client_timeout_secs: u64,
    // Default volume pool (pre-warms volumes for fast VM creation)
    pub default_volume_pool_enabled: bool,
    pub default_volume_pool_size: usize,
    // Misc VM config
    #[schema(value_type = String)]
    pub vm_root_drive_path: PathBuf,
    pub vm_user_name: String,
    pub vm_boot_timeout_secs: u64,
    // S3 config
    pub aws_commit_bucket_name: String,
    pub aws_sleep_snapshot_bucket_name: String,
    // API config
    pub image_upload_max_body_bytes: usize,
    // Agent binary path (optional — auto-discovered if not set)
    #[schema(value_type = Option<String>)]
    pub agent_binary_path: Option<PathBuf>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrchestratorConfig {
    pub wg_public_key: String,
    #[schema(value_type = String)]
    pub public_ip: IpAddr,
    #[schema(value_type = String)]
    pub wg_private_ip: Ipv6Addr,
    pub wg_port: u16,
    pub host: String,
    pub port: u16,
    pub admin_api_key: String,
    #[schema(value_type = String)]
    pub path: PathBuf,
    pub log_to_disk: bool,
    #[schema(value_type = String)]
    pub log_dir: PathBuf,
    pub usage_reporting_enabled: bool,
    pub usage_reporting_test_interval_secs: Option<u64>,
    // Various timeouts
    pub node_proto_request_timeout_secs: u64,
    pub incoming_request_timeout_secs: u64,
    pub health_check_timeout_secs: u64,
    pub action_timeout_secs: u64,
    pub task_shutdown_timeout_secs: u64,
    pub file_upload_timeout_secs: u64,
    pub vm_reconciliation_interval_secs: u64,
    // GitHub App credentials for deploy from GitHub
    pub github_app_id: Option<String>,
    pub github_app_private_key: Option<String>,
    // Stripe billing integration (optional — billing disabled if not set)
    pub stripe_secret_key: Option<String>,
    pub stripe_webhook_secret: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProxyConfig {
    // HTTP server config
    pub port: u16,
    #[schema(value_type = String)]
    pub interface: Ipv4Addr,
    // Admin API config
    #[schema(value_type = String)]
    pub admin_interface: Ipv4Addr,
    pub admin_port: u16,
    pub admin_api_key: String,
    // WireGuard config
    pub wg_private_key: String,
    pub wg_public_key: String,
    #[schema(value_type = String)]
    pub wg_private_ip: Ipv6Addr,
    #[schema(value_type = String)]
    pub public_ip: Ipv4Addr,
    pub wg_port: u16,
    // SSH-over-TLS config
    pub ssh_port: u16,
    #[schema(value_type = String)]
    pub ssh_cert_path: PathBuf,
    #[schema(value_type = String)]
    pub ssh_key_path: PathBuf,
    pub ssh_tls_handshake_timeout_secs: u64,
    pub ssh_backend_connect_timeout_secs: u64,
    pub ssh_idle_timeout_secs: u64,
    pub orch_forward_timeout_secs: u64,
    pub pool_manager_url: String,
    pub pool_manager_auth_token: String,
    // acme protocol secrets/values. used for gen TLS certs.
    pub acme_email: String,
    pub acme_account_key: String,
    pub acme_directory_url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MulberryConfig {
    // Task intervals
    pub ghost_vm_check_interval_seconds: u64,
    pub host_resource_check_interval_seconds: u64,
    pub orphan_wg_check_interval_seconds: u64,
    pub ghost_vm_fail_count_restart_threshold: u8,

    // Thresholds for host resource check tasks
    pub cpu_usage_warning_threshold: f32,
    pub memory_and_swap_usage_warning_threshold: f32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VersConfig {
    pub common: CommonConfig,
    pub chelsea: ChelseaConfig,
    pub orchestrator: OrchestratorConfig,
    pub proxy: ProxyConfig,
    pub mulberry: MulberryConfig,
}

impl VersConfig {
    /// Creates a new VersConfig struct from the default config dir defined at vers_config::DEFAULT_CONFIG_DIR.
    fn create_from_default_config_dir() -> Result<Self, String> {
        let mut config_dict = match get_default_config_dictionary() {
            Ok(dict) => dict,
            Err(err) => return Err(err.to_string()),
        };

        let mut missing_keys: Vec<String> = Vec::new();
        let mut invalid_keys: Vec<String> = Vec::new();

        // ************************
        // ** Convenience macros **
        // ************************

        // Get value from config_dict; if not present, push key to missing_keys
        macro_rules! dict_get {
            ($key:expr) => {{
                match config_dict.remove($key) {
                    Some(val) => val,
                    None => {
                        missing_keys.push($key.to_string());
                        String::new()
                    }
                }
            }};
        }

        // dict_get_parse - uses dict_get from above to get a String, then parses, pushing key to invalid_keys on error.
        macro_rules! dict_get_parse {
            ($key:expr, $t:ty) => {{
                let s = dict_get!($key);
                match <$t as std::str::FromStr>::from_str(&s) {
                    Ok(parsed) => parsed,
                    Err(_) => {
                        if !missing_keys.contains(&$key.to_string()) {
                            invalid_keys.push($key.to_string());
                        }
                        <$t>::default()
                    }
                }
            }};
        }

        // Special macro for PathBuf since it doesn't implement FromStr
        macro_rules! dict_get_path {
            ($key:expr) => {{
                let s = dict_get!($key);
                if s.is_empty() && missing_keys.contains(&$key.to_string()) {
                    PathBuf::new()
                } else {
                    PathBuf::from(s)
                }
            }};
        }

        // Special macro for Ipv4Addr since it doesn't implement Default
        macro_rules! dict_get_ipv4 {
            ($key:expr) => {{
                let s = dict_get!($key);
                match s.parse::<Ipv4Addr>() {
                    Ok(addr) => addr,
                    Err(_) => {
                        if !missing_keys.contains(&$key.to_string()) {
                            invalid_keys.push($key.to_string());
                        }
                        Ipv4Addr::UNSPECIFIED
                    }
                }
            }};
        }

        // Special macro for Ipv6Addr since it doesn't implement Default
        macro_rules! dict_get_ipv6 {
            ($key:expr) => {{
                let s = dict_get!($key);
                match s.parse::<Ipv6Addr>() {
                    Ok(addr) => addr,
                    Err(_) => {
                        if !missing_keys.contains(&$key.to_string()) {
                            invalid_keys.push($key.to_string());
                        }
                        Ipv6Addr::UNSPECIFIED
                    }
                }
            }};
        }

        // Special macro for IpAddr since it doesn't implement Default
        macro_rules! dict_get_ip {
            ($key:expr) => {{
                let s = dict_get!($key);
                match s.parse::<IpAddr>() {
                    Ok(addr) => addr,
                    Err(_) => {
                        if !missing_keys.contains(&$key.to_string()) {
                            invalid_keys.push($key.to_string());
                        }
                        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
                    }
                }
            }};
        }

        // ************
        // ** Common **
        // ************

        let pagerduty_alert_routing_key = config_dict
            .remove("pagerduty_alert_routing_key")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        if pagerduty_alert_routing_key.is_none() {
            warn!("`pagerduty_alert_routing_key` missing; PagerDuty alerts disabled");
        }

        let common = CommonConfig {
            // (PG) Database config
            database_url: dict_get!("database_url"),

            // Alerting config
            discord_alert_webhook_url: dict_get!("orchestrator_discord_alert_webhook_url"),
            pagerduty_alert_routing_key,
        };

        // *************
        // ** Chelsea **
        // *************

        // SSH port range (need start and end)
        let vm_ssh_port_start = dict_get_parse!("chelsea_vm_ssh_port_start", u16);
        let vm_ssh_port_end = dict_get_parse!("chelsea_vm_ssh_port_end", u16);

        // Data dir and subdirectories
        let data_dir = dict_get_path!("chelsea_data_dir");

        let db_subdir = dict_get!("chelsea_db_subdir");
        let db_name = dict_get!("chelsea_db_name");
        let monitoring_log_subdir = dict_get!("chelsea_monitoring_log_subdir");
        let process_log_subdir = dict_get!("chelsea_process_log_subdir");
        let kernel_subdir = dict_get!("chelsea_kernel_subdir");
        let snapshot_subdir = dict_get!("chelsea_commit_subdir");

        let chelsea = ChelseaConfig {
            // Networking
            event_server_addr: dict_get_ipv4!("chelsea_event_server_addr"),
            event_server_port: dict_get_parse!("chelsea_event_server_port", u16),
            server_port: dict_get_parse!("chelsea_server_port", u16),

            // Wireguard
            wg_interface_name: dict_get!("chelsea_wg_interface_name"),
            wg_port: dict_get_parse!("chelsea_wg_port", u16),

            // Data directories
            monitoring_log_dir: data_dir.join(monitoring_log_subdir),
            timing_log_target: dict_get!("chelsea_timing_log_subdir"),
            process_log_dir: data_dir.join(process_log_subdir),
            kernel_dir: data_dir.join(kernel_subdir),
            snapshot_dir: data_dir.join(snapshot_subdir),
            db_path: data_dir.join(db_subdir).join(db_name),
            data_dir,

            // Resource limits
            snapshot_dir_max_size_mib: dict_get_parse!("chelsea_snapshot_dir_max_size_mib", u32),

            // VM provisioning parameters
            vm_max_vcpu_count: dict_get_parse!("chelsea_vm_max_vcpu_count", u32),
            vm_max_memory_mib: dict_get_parse!("chelsea_vm_max_memory_mib", u32),
            vm_max_volume_mib: dict_get_parse!("chelsea_vm_max_volume_mib", u32),
            allow_vcpu_overprovisioning: dict_get_parse!(
                "chelsea_allow_vcpu_overprovisioning",
                bool
            ),
            allow_memory_overprovisioning: dict_get_parse!(
                "chelsea_allow_memory_overprovisioning",
                bool
            ),
            vm_total_vcpu_count: dict_get_parse!("chelsea_vm_total_vcpu_count", u32),
            vm_total_memory_mib: dict_get_parse!("chelsea_vm_total_memory_mib", u32),

            // VM config defaults
            vm_default_image_name: dict_get!("chelsea_vm_default_image_name"),
            vm_default_kernel_name: dict_get!("chelsea_vm_default_kernel_name"),
            vm_default_vcpu_count: dict_get_parse!("chelsea_vm_default_vcpu_count", u32),
            vm_default_mem_size_mib: dict_get_parse!("chelsea_vm_default_mem_size_mib", u32),
            vm_default_fs_size_mib: dict_get_parse!("chelsea_vm_default_fs_size_mib", u32),

            // Network manager config
            vm_subnet: dict_get_parse!("chelsea_vm_subnet", Ipv4Net),
            vm_ssh_port_range: vm_ssh_port_start..vm_ssh_port_end,
            network_reserve_timeout_secs: dict_get_parse!(
                "chelsea_network_reserve_timeout_seconds",
                u16
            ),

            // Firecracker-specific
            firecracker_bin_path: dict_get_path!("chelsea_firecracker_bin_path"),
            firecracker_api_timeout_secs: dict_get_parse!(
                "chelsea_firecracker_api_timeout_seconds",
                u64
            ),
            firecracker_socket_timeout_secs: dict_get_parse!(
                "chelsea_firecracker_socket_timeout_seconds",
                u64
            ),
            firecracker_use_huge_pages: config_dict
                .remove("chelsea_firecracker_use_huge_pages")
                .map(|v| {
                    let v = v.to_lowercase();
                    v == "true" || v == "1" || v == "yes"
                })
                .unwrap_or(false),

            // Cloud Hypervisor-specific
            cloud_hypervisor_bin_path: config_dict
                .remove("chelsea_cloud_hypervisor_bin_path")
                .map(PathBuf::from),

            // Hypervisor selection and per-hypervisor kernel names
            hypervisor_type: dict_get_parse!("chelsea_hypervisor_type", HypervisorType),
            firecracker_kernel_name: config_dict
                .remove("chelsea_firecracker_kernel_name")
                .unwrap_or_else(|| "firecracker.bin".to_string()),
            cloud_hypervisor_kernel_name: config_dict
                .remove("chelsea_cloud_hypervisor_kernel_name")
                .unwrap_or_else(|| "ch.bin".to_string()),
            // Cgroup isolation for VM processes
            vm_cgroup_name: config_dict
                .remove("chelsea_vm_cgroup_name")
                .unwrap_or_else(|| "chelsea-vms".to_string()),
            vm_cgroup_cpu_weight: config_dict
                .remove("chelsea_vm_cgroup_cpu_weight")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(50),

            // Ceph
            ceph_base_image_snap_name: dict_get!("chelsea_ceph_base_image_snap_name"),
            ceph_client_timeout_secs: dict_get_parse!("chelsea_ceph_client_timeout_seconds", u64),

            // Default volume pool
            default_volume_pool_enabled: config_dict
                .remove("chelsea_default_volume_pool_enabled")
                .map(|v| {
                    let v = v.to_lowercase();
                    v == "true" || v == "1" || v == "yes"
                })
                .unwrap_or(true),
            default_volume_pool_size: config_dict
                .remove("chelsea_default_volume_pool_size")
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),

            // Misc VM config
            vm_root_drive_path: dict_get_path!("chelsea_vm_root_drive_path"),
            vm_user_name: dict_get!("chelsea_vm_user_name"),
            vm_boot_timeout_secs: dbg!(dict_get_parse!("chelsea_vm_boot_timeout_seconds", u64)),

            // S3 config
            aws_commit_bucket_name: dict_get!("chelsea_aws_commit_bucket_name"),
            aws_sleep_snapshot_bucket_name: dict_get!("chelsea_aws_sleep_snapshot_bucket_name"),

            // API config
            image_upload_max_body_bytes: dict_get_parse!(
                "chelsea_image_upload_max_body_bytes",
                usize
            ),

            // Agent binary path (optional — auto-discovered from exe dir / target/ if not set)
            agent_binary_path: config_dict
                .remove("chelsea_agent_binary_path")
                .map(PathBuf::from),
        };

        // ******************
        // ** Orchestrator **
        // ******************

        let usage_reporting_test_interval_secs = {
            const KEY: &str = "orchestrator_usage_reporting_test_interval_seconds";
            match config_dict.remove(KEY) {
                None => None,
                Some(val) => {
                    let v = val.trim();
                    if v.is_empty() {
                        None
                    } else {
                        match v.parse::<u64>() {
                            Ok(0) => None,
                            Ok(n) => Some(n),
                            Err(_) => {
                                invalid_keys.push(KEY.to_string());
                                None
                            }
                        }
                    }
                }
            }
        };

        let orchestrator_path = dict_get_path!("orchestrator_path");

        let orchestrator = OrchestratorConfig {
            wg_public_key: dict_get!("orchestrator_wg_public_key"),
            public_ip: dict_get_ip!("orchestrator_public_ip"),
            wg_private_ip: dict_get_ipv6!("orchestrator_wg_private_ip"),
            wg_port: dict_get_parse!("orchestrator_wg_port", u16),
            host: dict_get!("orchestrator_host"),
            admin_api_key: dict_get!("orchestrator_admin_api_key"),

            port: dict_get_parse!("orchestrator_port", u16),
            path: orchestrator_path.clone(),
            log_to_disk: dict_get_parse!("orchestrator_log_to_disk", bool),
            log_dir: orchestrator_path.join("logs"),
            usage_reporting_enabled: dict_get_parse!("orchestrator_usage_reporting_enabled", bool),
            usage_reporting_test_interval_secs,

            node_proto_request_timeout_secs: dict_get_parse!(
                "orchestrator_node_proto_request_timeout_seconds",
                u64
            ),
            incoming_request_timeout_secs: dict_get_parse!(
                "orchestrator_incoming_request_timeout_seconds",
                u64
            ),
            health_check_timeout_secs: dict_get_parse!(
                "orchestrator_health_check_timeout_seconds",
                u64
            ),
            action_timeout_secs: dict_get_parse!("orchestrator_action_timeout_seconds", u64),
            task_shutdown_timeout_secs: dict_get_parse!(
                "orchestrator_task_shutdown_timeout_seconds",
                u64
            ),
            file_upload_timeout_secs: dict_get_parse!(
                "orchestrator_file_upload_timeout_seconds",
                u64
            ),
            vm_reconciliation_interval_secs: config_dict
                .remove("orchestrator_vm_reconciliation_interval_seconds")
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            github_app_id: config_dict.remove("orchestrator_github_app_id"),
            github_app_private_key: config_dict.remove("orchestrator_github_app_private_key"),
            stripe_secret_key: config_dict.remove("stripe_secret_key"),
            stripe_webhook_secret: config_dict.remove("stripe_webhook_secret"),
        };

        // ***********
        // ** Proxy **
        // ***********

        let proxy = ProxyConfig {
            // HTTP server config
            port: dict_get_parse!("proxy_port", u16),
            interface: dict_get_ipv4!("proxy_interface"),

            // Admin API config
            admin_interface: dict_get_ipv4!("proxy_admin_interface"),
            admin_port: dict_get_parse!("proxy_admin_port", u16),
            admin_api_key: dict_get!("proxy_admin_api_key"),

            // WireGuard config
            wg_private_key: dict_get!("proxy_wg_private_key"),
            wg_public_key: dict_get!("proxy_wg_public_key"),
            wg_private_ip: dict_get_ipv6!("proxy_wg_private_ip"),
            public_ip: dict_get_ipv4!("proxy_public_ip"),
            wg_port: dict_get_parse!("proxy_wg_port", u16),

            // SSH-over-TLS config
            ssh_port: dict_get_parse!("proxy_ssh_port", u16),
            ssh_cert_path: dict_get_path!("proxy_ssh_cert_path"),
            ssh_key_path: dict_get_path!("proxy_ssh_key_path"),
            ssh_tls_handshake_timeout_secs: dict_get_parse!(
                "proxy_ssh_tls_handshake_timeout_seconds",
                u64
            ),
            ssh_backend_connect_timeout_secs: dict_get_parse!(
                "proxy_ssh_backend_connect_timeout_seconds",
                u64
            ),
            ssh_idle_timeout_secs: dict_get_parse!("proxy_ssh_idle_timeout_seconds", u64),
            orch_forward_timeout_secs: dict_get_parse!("proxy_orch_forward_timeout_seconds", u64),
            pool_manager_url: dict_get!("proxy_pool_manager_url"),
            pool_manager_auth_token: dict_get!("proxy_pool_auth_token"),
            acme_email: dict_get!("proxy_acme_email"),
            acme_account_key: dict_get!("proxy_acme_account_key"),
            acme_directory_url: dict_get!("proxy_acme_directory_url"),
        };

        // **************
        // ** Mulberry **
        // **************

        let mulberry = MulberryConfig {
            ghost_vm_check_interval_seconds: dict_get_parse!(
                "mulberry_ghost_vm_check_interval_seconds",
                u64
            ),
            host_resource_check_interval_seconds: dict_get_parse!(
                "mulberry_host_resource_check_interval_seconds",
                u64
            ),
            orphan_wg_check_interval_seconds: dict_get_parse!(
                "mulberry_orphan_wg_check_interval_seconds",
                u64
            ),
            ghost_vm_fail_count_restart_threshold: dict_get_parse!(
                "mulberry_ghost_vm_fail_count_restart_threshold",
                u8
            ),
            cpu_usage_warning_threshold: dict_get_parse!(
                "mulberry_cpu_usage_warning_threshold",
                f32
            ),
            memory_and_swap_usage_warning_threshold: dict_get_parse!(
                "mulberry_memory_and_swap_mib_usage_warning_threshold",
                f32
            ),
        };

        let config = Self {
            common,
            chelsea,
            orchestrator,
            proxy,
            mulberry,
        };

        // Check if any unused keys remain in the config dict; warn if so.
        let unused_keys = config_dict
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<&str>>();
        if !unused_keys.is_empty() {
            warn!("One or more unused config vars: {}", unused_keys.join(", "));
        }

        // Only return Ok if there were no errors; error types aren't used here for the sake of cleanliness.
        if missing_keys.is_empty() && invalid_keys.is_empty() {
            Ok(config)
        } else {
            Err(format!(
                "One or more config errors while reading config from {}. Missing: [{}], Invalid: [{}]",
                DEFAULT_CONFIG_DIR,
                missing_keys.join(", "),
                invalid_keys.join(", ")
            ))
        }
    }

    /// Returns a reference to the global singleton `VersConfig` instance.
    pub fn global() -> &'static Self {
        static_vers_config()
    }

    /// Returns a reference to the common/shared configuration section.
    pub fn common() -> &'static CommonConfig {
        &Self::global().common
    }

    /// Returns a reference to the Chelsea-specific configuration section.
    pub fn chelsea() -> &'static ChelseaConfig {
        &Self::global().chelsea
    }

    /// Returns a reference to the Orchestrator-specific configuration section.
    pub fn orchestrator() -> &'static OrchestratorConfig {
        &Self::global().orchestrator
    }

    /// Returns a reference to the Proxy-specific configuration section.
    pub fn proxy() -> &'static ProxyConfig {
        &Self::global().proxy
    }

    /// Returns a reference to the Mulberry-specific configuration section.
    pub fn mulberry() -> &'static MulberryConfig {
        &Self::global().mulberry
    }
}

fn get_default_config_dir() -> &'static Path {
    Path::new(DEFAULT_CONFIG_DIR)
}

#[derive(Debug, Error)]
pub enum InitializeConfigError {
    #[error("Error while getting config sources: {0}")]
    GetSources(#[from] GetSourcesError),
}

/// Gets the config dictionary using the const DEFAULT_CONFIG_DIR as the config directory. Contains all the raw key-value
/// pairs read from all local and remote sources.
fn get_default_config_dictionary() -> Result<HashMap<String, String>, InitializeConfigError> {
    get_config_dictionary(get_default_config_dir()).map_err(InitializeConfigError::from)
}
