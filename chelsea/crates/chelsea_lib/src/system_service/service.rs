use tokio::sync::Mutex;

use sysinfo::{CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

pub struct SystemService {
    system: Mutex<System>,
    disks: Mutex<Disks>,
}

impl SystemService {
    /// Returns the total and available RAM in MiB
    pub async fn get_total_and_available_ram(&self) -> (u64, u64) {
        let mut sys = self.system.lock().await;

        sys.refresh_memory();

        let total_mib = sys.total_memory() / (1024 * 1024);
        let available_mib = sys.available_memory() / (1024 * 1024);

        (total_mib, available_mib)
    }

    /// Returns the total and available CPU%
    pub async fn get_total_and_available_cpu(&self) -> (f64, f64) {
        let mut sys = self.system.lock().await;

        sys.refresh_cpu_usage();

        let global_cpu = sys.global_cpu_usage();
        let total = 100.0;
        let available = 100.0 - global_cpu as f64;

        (total, available)
    }

    /// Returns the total vCPU count
    pub async fn get_vcpu_count(&self) -> u64 {
        let mut sys = self.system.lock().await;

        sys.refresh_cpu_usage();

        sys.cpus().len() as u64
    }

    /// Returns the total and available disk space in MiB
    pub async fn get_total_and_available_disk(&self) -> (u64, u64) {
        let mut disks = self.disks.lock().await;

        disks.refresh_specifics(true, DiskRefreshKind::nothing().with_storage());

        let mut total = 0u64;
        let mut available = 0u64;

        for disk in disks.iter() {
            total += disk.total_space();
            available += disk.available_space();
        }

        let total_mib = total / 1024 / 1024;
        let available_mib = available / 1024 / 1024;

        (total_mib, available_mib)
    }
}

impl Default for SystemService {
    fn default() -> Self {
        let system = Mutex::new(System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::everything()),
        ));

        let disks = Mutex::new(Disks::new_with_refreshed_list_specifics(
            DiskRefreshKind::nothing().with_storage(),
        ));

        Self { system, disks }
    }
}
