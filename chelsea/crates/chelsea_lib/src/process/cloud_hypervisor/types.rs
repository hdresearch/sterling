use serde::{Deserialize, Serialize};
use vers_config::VersConfig;

/// The struct returned from the vm.info API call
#[derive(Serialize, Deserialize, Debug)]
pub struct CloudHypervisorVmInfo {
    pub config: CloudHypervisorVmConfig,
    pub state: CloudHypervisorVmState,
}

/// Cloud Hypervisor VM state
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum CloudHypervisorVmState {
    Created,
    Running,
    Shutdown,
    Paused,
}

/// Cloud Hypervisor VM configuration (simplified for basic lifecycle)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CloudHypervisorVmConfig {
    /// CPU configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<CpusConfig>,

    /// Memory configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryConfig>,

    /// Payload configuration (kernel or firmware)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<PayloadConfig>,

    /// Disk configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disks: Option<Vec<DiskConfig>>,

    /// Network configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<Vec<NetConfig>>,

    /// Serial console configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<SerialConfig>,

    /// Console configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console: Option<ConsoleConfig>,

    /// Vsock configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vsock: Option<VsockConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PayloadConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmdline: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CpusConfig {
    pub boot_vcpus: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_vcpus: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MemoryConfig {
    pub size: u64, // Size in bytes
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct KernelConfig {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmdline: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DiskConfig {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    /// Enable direct I/O - required for raw block devices (RBD)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct: Option<bool>,
    /// Disk identifier used by cloud-hypervisor for resize operations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NetConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_queues: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SerialConfig {
    pub file: Option<String>,
    pub mode: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ConsoleConfig {
    pub mode: String,
}

/// Vsock device configuration for host-guest communication
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VsockConfig {
    /// Guest Context ID (must be >= 3)
    pub cid: u64,
    /// Path to the Unix domain socket on the host for vsock connections
    pub socket: String,
    /// Optional device ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Configuration for vm.snapshot API call
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmSnapshotConfig {
    /// URL to the snapshot destination (e.g., "file:///path/to/dir")
    pub destination_url: String,
}

/// Configuration for vm.restore API call
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmRestoreConfig {
    /// URL to the snapshot source (e.g., "file:///path/to/dir")
    pub source_url: String,
    /// Whether to prefault memory on restore
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefault: Option<bool>,
}

/// Configuration for vm.resize API call (CPU/memory hotplug)
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct VmResizeConfig {
    /// Desired CPU configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<CpusConfig>,
    /// Desired memory configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryConfig>,
    /// Desired memory zone configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_zones: Option<Vec<MemoryZoneConfig>>,
}

/// Configuration for vm.resize-disk API call
/// This notifies the guest kernel that a block device has grown.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmResizeDiskConfig {
    /// Disk ID (e.g., "_disk0" - the ID assigned by cloud-hypervisor)
    pub id: String,
    /// Desired size in bytes
    pub desired_size: u64,
}

/// Configuration for a memory zone (for hotplug)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryZoneConfig {
    /// Memory zone ID
    pub id: String,
    /// Memory size in MiB
    pub size_mib: u64,
}

impl Default for CloudHypervisorVmConfig {
    fn default() -> Self {
        Self {
            cpus: Some(CpusConfig {
                boot_vcpus: VersConfig::chelsea().vm_default_vcpu_count,
                max_vcpus: None,
            }),
            memory: Some(MemoryConfig {
                size: (VersConfig::chelsea().vm_default_mem_size_mib as u64) * 1024 * 1024,
            }),
            payload: None,
            disks: None,
            net: None,
            serial: None,
            console: None,
            vsock: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_state_serialization() {
        let state = CloudHypervisorVmState::Running;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"Running\"");

        let state = CloudHypervisorVmState::Created;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"Created\"");
    }

    #[test]
    fn test_vm_state_deserialization() {
        let json = "\"Running\"";
        let state: CloudHypervisorVmState = serde_json::from_str(json).unwrap();
        assert_eq!(state, CloudHypervisorVmState::Running);
    }

    #[test]
    fn test_cpus_config_serialization() {
        let config = CpusConfig {
            boot_vcpus: 4,
            max_vcpus: Some(8),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"boot_vcpus\":4"));
        assert!(json.contains("\"max_vcpus\":8"));
    }

    #[test]
    fn test_memory_config_conversion() {
        let config = MemoryConfig {
            size: 2048 * 1024 * 1024, // 2048 MiB in bytes
        };
        assert_eq!(config.size, 2147483648);
    }

    #[test]
    fn test_kernel_config_with_cmdline() {
        let config = KernelConfig {
            path: "/path/to/kernel".to_string(),
            cmdline: Some("console=ttyS0".to_string()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("console=ttyS0"));
    }

    #[test]
    fn test_disk_config_readonly() {
        let config = DiskConfig {
            path: "/dev/vda".to_string(),
            readonly: Some(true),
            direct: None,
            id: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"readonly\":true"));
    }

    #[test]
    fn test_disk_config_direct_io() {
        let config = DiskConfig {
            path: "/dev/rbd0".to_string(),
            readonly: Some(false),
            direct: Some(true),
            id: Some("_disk0".to_string()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"direct\":true"));
        assert!(json.contains("\"id\":\"_disk0\""));
    }

    #[test]
    fn test_net_config_with_tap_and_mac() {
        let config = NetConfig {
            tap: Some("tap0".to_string()),
            mac: Some("06:00:00:00:00:01".to_string()),
            num_queues: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("tap0"));
        assert!(json.contains("06:00:00:00:00:01"));
    }

    #[test]
    fn test_vm_config_complete() {
        let config = CloudHypervisorVmConfig {
            cpus: Some(CpusConfig {
                boot_vcpus: 2,
                max_vcpus: None,
            }),
            memory: Some(MemoryConfig {
                size: 1024 * 1024 * 1024,
            }),
            payload: Some(PayloadConfig {
                kernel: Some("/vmlinux".to_string()),
                cmdline: Some("console=ttyS0".to_string()),
            }),
            disks: Some(vec![DiskConfig {
                path: "/dev/vda".to_string(),
                readonly: Some(false),
                direct: None,
                id: None,
            }]),
            net: Some(vec![NetConfig {
                tap: Some("tap0".to_string()),
                mac: Some("06:00:00:00:00:01".to_string()),
                num_queues: None,
            }]),
            serial: None,
            console: None,
            vsock: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: CloudHypervisorVmConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.cpus.as_ref().unwrap().boot_vcpus, 2);
        assert_eq!(
            deserialized.memory.as_ref().unwrap().size,
            1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_vm_config_optional_fields() {
        let config = CloudHypervisorVmConfig {
            cpus: Some(CpusConfig {
                boot_vcpus: 1,
                max_vcpus: None,
            }),
            memory: Some(MemoryConfig {
                size: 512 * 1024 * 1024,
            }),
            payload: None,
            disks: None,
            net: None,
            serial: None,
            console: None,
            vsock: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        // Optional None fields should not appear in JSON
        assert!(!json.contains("payload"));
        assert!(!json.contains("disks"));
        assert!(!json.contains("net"));
    }
}
