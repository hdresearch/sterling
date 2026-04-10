use std::sync::{Mutex, MutexGuard, OnceLock};

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
fn system() -> MutexGuard<'static, System> {
    SYSTEM
        .get_or_init(|| {
            Mutex::new(System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                    .with_memory(MemoryRefreshKind::everything()),
            ))
        })
        .lock()
        .expect("Failed to lock SYSTEM mutex")
}

// static DISKS: OnceLock<Mutex<Disks>> = OnceLock::new();
// fn disks() -> MutexGuard<'static, Disks> {
//     DISKS
//         .get_or_init(|| {
//             Mutex::new(Disks::new_with_refreshed_list_specifics(
//                 DiskRefreshKind::nothing().with_storage(),
//             ))
//         })
//         .lock()
//         .expect("Failed to lock DISKS mutex")
// }

pub fn get_vcpu_count_total() -> u32 {
    let sys = system();
    sys.cpus().len() as u32
}

pub fn get_mem_size_mib_total() -> u32 {
    let mut sys = system();
    sys.refresh_memory();
    (sys.total_memory() / (1024 * 1024)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcpu_count_is_positive() {
        let count = get_vcpu_count_total();
        assert!(count > 0, "expected at least 1 vCPU, got {count}");
    }

    #[test]
    fn mem_size_is_positive() {
        let mib = get_mem_size_mib_total();
        assert!(mib > 0, "expected at least 1 MiB of memory, got {mib}");
    }

    #[test]
    fn vcpu_count_is_reasonable() {
        let count = get_vcpu_count_total();
        // No machine should report more than 4096 cores
        assert!(count <= 4096, "vCPU count {count} seems unreasonably large");
    }

    #[test]
    fn mem_size_is_reasonable() {
        let mib = get_mem_size_mib_total();
        // No machine should report more than 16 TiB
        assert!(
            mib <= 16 * 1024 * 1024,
            "memory {mib} MiB seems unreasonably large"
        );
    }
}
