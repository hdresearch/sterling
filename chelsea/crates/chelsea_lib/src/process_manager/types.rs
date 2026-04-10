use crate::process::cloud_hypervisor::config::CloudHypervisorProcessConfig;
use crate::process::firecracker::config::FirecrackerProcessConfig;

/// Represents config variants for VM backends a ProcessManager is capable of spawning
pub enum VmProcessConfig {
    Firecracker(FirecrackerProcessConfig),
    CloudHypervisor(CloudHypervisorProcessConfig),
}
