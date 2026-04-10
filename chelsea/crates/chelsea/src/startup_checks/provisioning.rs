use anyhow::bail;
use tracing::warn;
use util::system_info::{get_mem_size_mib_total, get_vcpu_count_total};
use vers_config::VersConfig;

// Warning thresholds defined by R-27788-31827-10003-53341-01793-53542-02039-28664
const VCPU_COUNT_DELTA_WARNING_THRESHOLD: u32 = 4;
const MEMORY_MIB_COUNT_DELTA_WARNING_THRESHOLD: u32 = 8192;

/// Ensure that the parameters related to VM provisioning are valid.
pub fn validate_vm_provisioning_parameters() -> anyhow::Result<()> {
    let config = VersConfig::chelsea();

    // If overprovisioning is disabled, verify that the host machine's capabilities are not exceeded by the configured VM provisioning capabilities.
    if !config.allow_vcpu_overprovisioning {
        let host_vcpu_count = get_vcpu_count_total();
        let vm_total_vcpu_count = config.vm_total_vcpu_count;

        let delta = host_vcpu_count.saturating_sub(vm_total_vcpu_count);
        if delta == 0 {
            bail!(
                "Cannot set VM vCPU count allocation to {vm_total_vcpu_count}; host only has {host_vcpu_count} vCPUs. To ignore this error, set chelsea_allow_vcpu_overprovisioning=true."
            );
        } else if delta < VCPU_COUNT_DELTA_WARNING_THRESHOLD {
            warn!(
                "Host has {host_vcpu_count} vCPUs; setting chelsea_vm_total_vcpu_count to {vm_total_vcpu_count} would leave host with fewer than {VCPU_COUNT_DELTA_WARNING_THRESHOLD} for non-VM processes."
            )
        }
    }

    if !config.allow_memory_overprovisioning {
        let host_memory_mib = get_mem_size_mib_total();
        let vm_total_memory_mib = config.vm_total_memory_mib;

        let delta = host_memory_mib.saturating_sub(vm_total_memory_mib);
        if delta == 0 {
            bail!(
                "Cannot set VM memory allocation to {vm_total_memory_mib} MiB; host only has {host_memory_mib} MiB. set chelsea_allow_memory_overprovisioning=true"
            );
        } else if delta < MEMORY_MIB_COUNT_DELTA_WARNING_THRESHOLD {
            warn!(
                "Host has {host_memory_mib} MiB total memory; setting chelsea_vm_total_memory_mib to {vm_total_memory_mib} would leave host with fewer than {MEMORY_MIB_COUNT_DELTA_WARNING_THRESHOLD} MiB for non-VM processes."
            )
        }
    }

    Ok(())
}
