use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The system's RAM totals/usage; all units are MiB
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone, Copy)]
pub struct TelemetryRam {
    /// The total amount of RAM available on the system
    pub real_mib_total: u64,
    /// The amount of RAM stated by the system to be available
    pub real_mib_available: u64,
    /// The total amount of RAM "reserved" for VMs
    pub vm_mib_total: u32,
    /// The total, minus theoretical RAM consumption of VMs
    pub vm_mib_available: u32,
}
/// The system's CPU totals/usage; all units are percentages, ie: 35.32 = 35.32%, or vCPU counts (for VMs)
/// for consistency
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone, Copy)]
pub struct TelemetryCpu {
    /// The total %CPU available on the system (likely 100.00%)
    pub real_total: f64,
    /// The %CPU stated by the system to be available
    pub real_available: f64,
    /// The total vCPU count
    pub vcpu_count_total: u64,
    /// The vCPU count "reserved" for VMs
    pub vcpu_count_vm_total: u32,
    /// The total, minus theoretical vCPU consumption of VMs
    pub vcpu_count_vm_available: u32,
}

/// The system's FS totals/usage; all units are MiB
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone, Copy)]
pub struct TelemetryFs {
    /// The total FS space available to the system
    pub mib_total: u64,
    /// The available FS space available to the system
    pub mib_available: u64,
}

/// Represents chelsea-specific stats
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone, Copy)]
pub struct TelemetryChelsea {
    /// The maximum number of VMs this node can support; currently tied to the number of VmNetworks allocated by the NetworkManager
    pub vm_count_max: u64,
    /// The number of VMs currently allocated on this node
    pub vm_count_current: u64,
}

/// The response body for GET /api/system/telemetry
#[derive(Serialize, Deserialize, ToSchema, Debug, Clone, Copy)]
pub struct SystemTelemetryResponse {
    pub ram: TelemetryRam,
    pub cpu: TelemetryCpu,
    pub fs: TelemetryFs,
    pub chelsea: TelemetryChelsea,
}

/// Response struct for GET /api/system/version
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ChelseaVersion {
    /// Executable identifier; should be "chelsea"
    pub executable_name: String,
    /// Current workspace version
    pub workspace_version: String,
    /// Current git hash used for the build
    pub git_hash: String,
    /// Jailer executable version
    pub jailer_version: String,
    /// Firecracker executable version
    pub firecracker_version: String,
    /// Ceph client executable version
    pub ceph_client_version: String,
    /// Ceph version reported by cluster
    pub ceph_cluster_version: String,
}
