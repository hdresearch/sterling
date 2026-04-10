use serde::{Deserialize, Serialize};
use vers_config::VersConfig;

/// The struct returned from the describeInstance API call ( GET / )
#[derive(Serialize, Deserialize, Debug)]
pub struct FirecrackerInstanceInfo {
    pub app_name: String,
    pub id: String,
    pub state: FirecrackerInstanceState,
    pub vmm_version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum FirecrackerInstanceState {
    #[serde(rename = "Not started")]
    NotStarted,
    Running,
    Paused,
}

/// The CPU Template defines a set of flags to be disabled from the microvm so that
/// the features exposed to the guest are the same as in the selected instance type.
/// This parameter has been deprecated and it will be removed in future Firecracker
/// release.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum CpuTemplate {
    C3,
    T2,
    T2S,
    T2CL,
    T2A,
    V1N1,
    None,
}

impl Default for CpuTemplate {
    fn default() -> Self {
        CpuTemplate::None
    }
}

/// Which huge pages configuration (if any) should be used to back guest memory.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum HugePages {
    None,
    #[serde(rename = "2M")]
    TwoM,
}

impl Default for HugePages {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MachineConfiguration {
    /// The CPU template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_template: Option<CpuTemplate>,

    /// Flag for enabling/disabling simultaneous multithreading. Can be enabled only on x86.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smt: Option<bool>,

    /// Memory size of VM (in MiB)
    pub mem_size_mib: u32,

    /// Enable dirty page tracking. If this is enabled, then incremental guest memory
    /// snapshots can be created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_dirty_pages: Option<bool>,

    /// Number of vCPUs (max 32)
    pub vcpu_count: u32,

    /// Which huge pages configuration (if any) should be used to back guest memory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub huge_pages: Option<HugePages>,
}

impl Default for MachineConfiguration {
    fn default() -> Self {
        let config = VersConfig::chelsea();
        Self {
            cpu_template: None,
            smt: None,
            mem_size_mib: config.vm_default_mem_size_mib,
            track_dirty_pages: None,
            vcpu_count: config.vm_default_vcpu_count,
            huge_pages: if config.firecracker_use_huge_pages {
                Some(HugePages::TwoM)
            } else {
                None
            },
        }
    }
}
