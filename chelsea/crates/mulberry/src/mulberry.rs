use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    sync::Arc,
    time::Duration,
};

use chelsea_lib::{
    network_manager::wireguard::{delete_wg_interface, list_orphaned_wg_interfaces},
    vm_manager::VmManager,
};
use sysinfo::{ProcessesToUpdate, System};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use vers_config::VersConfig;

use crate::utils::get_memory_and_swap_usage;

const FIRECRACKER_PROCESS_NAME: &str = "firecracker";
// Note: Linux /proc/{pid}/comm truncates process names to 15 characters,
// so "cloud-hypervisor" becomes "cloud-hyperviso"
const CLOUD_HYPERVISOR_PROCESS_NAME: &str = "cloud-hyperviso";

/// The host monitoring subsystem for chelsea.
pub struct Mulberry {
    system: Mutex<System>,
    vm_manager: Arc<VmManager>,
    node_id: Uuid,
}

impl Mulberry {
    pub fn new(vm_manager: Arc<VmManager>, node_id: &Uuid) -> Self {
        Self {
            system: Mutex::new(System::new()),
            vm_manager,
            node_id: node_id.clone(),
        }
    }

    /// Returns a list of VM PIDs (both Firecracker and Cloud Hypervisor) found running on the system
    async fn get_running_vm_pids(&self) -> HashSet<u32> {
        let mut system = self.system.lock().await;
        system.refresh_processes(ProcessesToUpdate::All, true);

        let firecracker_pids = system
            .processes_by_name(OsStr::new(FIRECRACKER_PROCESS_NAME))
            .map(|process| process.pid().as_u32());

        let cloud_hypervisor_pids = system
            .processes_by_name(OsStr::new(CLOUD_HYPERVISOR_PROCESS_NAME))
            .map(|process| process.pid().as_u32());

        firecracker_pids.chain(cloud_hypervisor_pids).collect()
    }

    /// Returns a list of VM IDs+PIDs found in the local sqlite DB
    async fn get_db_vms_with_pids(&self) -> Vec<(Uuid, u32)> {
        match self.vm_manager.local_store.list_all_vms_with_pids().await {
            Ok(vec) => vec,
            Err(error) => {
                error!(%error, "Failed to fetch list of VMs and PIDs from local store");
                Vec::new()
            }
        }
    }

    /// Returns a list of VM IDs that exist in the database, but for whom a process with the supposed PID could not be found
    async fn get_ghost_vms(&self) -> HashSet<Uuid> {
        let db_vms_with_pids = self.get_db_vms_with_pids().await;
        let running_pids = self.get_running_vm_pids().await;

        db_vms_with_pids
            .into_iter()
            .filter_map(|(vm_id, pid)| match running_pids.contains(&pid) {
                true => None,
                false => Some(vm_id),
            })
            .collect()
    }

    /// Begin monitoring for ghost VMs
    async fn start_ghost_vm_task(&self) {
        // Tracks the number of times a particular VM has failed the ghost check. When the count exceeds mulberry_ghost_vm_max_fail_count,
        // the VM will be restarted
        let mut consecutive_failure_count: HashMap<Uuid, u8> = HashMap::new();
        loop {
            let ghost_vms = self.get_ghost_vms().await;

            // Increment failure count for every found ghost VM
            for vm_id in ghost_vms.iter() {
                let entry = consecutive_failure_count.entry(*vm_id).or_insert(0);
                *entry += 1;
                debug!(%vm_id, failure_count = entry, "Found ghost VM; incrementing failure count");
            }

            // Remove VMs in the consecutive failure count map that weren't ghost VMs in this iteration
            let mut recovered_vms = HashSet::new();
            for (vm_id, failure_count) in consecutive_failure_count.iter() {
                if !ghost_vms.contains(vm_id) {
                    debug!(%vm_id, failure_count, "Previously-detected ghost VM is no longer a ghost VM; resetting failure count");
                    recovered_vms.insert(vm_id.clone());
                }
            }
            for vm_id in recovered_vms {
                consecutive_failure_count.remove(&vm_id);
            }

            // Attempt to restart VMs whose failure counts have matched or exceeded the threshold
            let mut restarted_vms = HashSet::new();
            for (vm_id, failure_count) in consecutive_failure_count.iter() {
                if *failure_count >= VersConfig::mulberry().ghost_vm_fail_count_restart_threshold {
                    info!(%vm_id, failure_count, "Ghost VM has exceeded consecutive failure check threshold; restarting");
                    if let Err(error) = self.vm_manager.reboot_or_delete_vm(&vm_id).await {
                        warn!(%error, "Error while deleting ghost VM (this may be expected)");
                    } else {
                        info!(%vm_id, failure_count, "Successfully restarted ghost VM; resetting failure count");
                        restarted_vms.insert(vm_id.clone());
                    }
                }
            }
            for vm_id in restarted_vms {
                consecutive_failure_count.remove(&vm_id);
            }

            tokio::time::sleep(Duration::from_secs(
                VersConfig::mulberry().ghost_vm_check_interval_seconds,
            ))
            .await;
        }
    }

    /// Returns the current CPU and Mem+Swap usage as percentages (pre-multiplied by 100). Mem+Swap usage is calculated
    /// by adding the memory and swap usages, in bytes, then dividing by the total amount of RAM available.
    async fn get_cpu_and_memswap_usage(&self) -> (f32, f32) {
        let mut system = self.system.lock().await;
        system.refresh_cpu_usage();
        system.refresh_memory();

        (
            system.global_cpu_usage(),
            get_memory_and_swap_usage(&system),
        )
    }

    /// Begin monitoring host resource usage
    async fn start_host_resource_task(&self) {
        loop {
            let (cpu_usage, memory_and_swap_usage) = self.get_cpu_and_memswap_usage().await;
            self.handle_cpu_and_memswap_usage(cpu_usage, memory_and_swap_usage)
                .await;

            tokio::time::sleep(Duration::from_secs(
                VersConfig::mulberry().host_resource_check_interval_seconds,
            ))
            .await;
        }
    }

    /// Monitor for orphaned WireGuard interfaces in the global namespace.
    ///
    /// In steady state, all VM WireGuard interfaces (`vm_*`) should be inside
    /// a network namespace. Any found in the global namespace are orphans from
    /// failed or interrupted `wg_setup` calls and are auto-deleted.
    async fn start_orphan_wg_task(&self) {
        loop {
            match list_orphaned_wg_interfaces() {
                Ok(orphans) if orphans.is_empty() => {
                    debug!("No orphaned WG interfaces in global namespace");
                }
                Ok(orphans) => {
                    warn!(
                        count = orphans.len(),
                        interfaces = ?orphans,
                        "Found orphaned WG interfaces in global namespace, cleaning up"
                    );
                    for name in &orphans {
                        match delete_wg_interface(name) {
                            Ok(()) => {
                                info!(interface = %name, "Deleted orphaned WG interface");
                            }
                            Err(e) => {
                                error!(
                                    interface = %name,
                                    error = %e,
                                    "Failed to delete orphaned WG interface"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to list WG interfaces for orphan check");
                }
            }

            tokio::time::sleep(Duration::from_secs(
                VersConfig::mulberry().orphan_wg_check_interval_seconds,
            ))
            .await;
        }
    }

    /// Starts all of Mulberry's monitoring tasks. Must be explicitly canceled; any errors or warnings will be logged rather than halting execution.
    pub async fn start_all(&self) {
        info!("Starting monitoring tasks");
        tokio::join!(
            self.start_ghost_vm_task(),
            self.start_host_resource_task(),
            self.start_orphan_wg_task()
        );
        info!("All monitoring tasks exited");
    }

    /// Compares the provided CPU and Mem+Swap usage percentages (pre-multiplied by 100) against the configured threshold values.
    /// If the warning threshold is exceeded, a warning will be logged, and an alert sent.
    async fn handle_cpu_and_memswap_usage(&self, cpu_usage: f32, memory_and_swap_usage: f32) {
        let config = VersConfig::mulberry();

        let cpu_current = cpu_usage;
        let cpu_threshold = config.cpu_usage_warning_threshold;
        if cpu_current > cpu_threshold {
            warn!(cpu_current, cpu_threshold, "CPU usage threshold exceeded");
            alerting::resource_threshold_exceeded(&self.node_id, "CPU", cpu_threshold, cpu_current)
                .await;
        }

        let ram_current = memory_and_swap_usage;
        let ram_threshold = config.memory_and_swap_usage_warning_threshold;
        if ram_current > ram_threshold {
            warn!(
                current = memory_and_swap_usage,
                threshold = config.memory_and_swap_usage_warning_threshold,
                "RAM+Swap usage threshold exceeded"
            );
            alerting::resource_threshold_exceeded(&self.node_id, "RAM", ram_threshold, ram_current)
                .await;
        }
    }
}
