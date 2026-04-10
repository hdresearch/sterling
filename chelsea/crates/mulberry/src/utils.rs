use sysinfo::System;

/// Returns the memory and swap usage of the provided system as a percentage of the total memory. For example, if
/// the current memory usage is 50 GiB / 100 GiB, this will return 50.0. If memory usage is 95 GiB / 100 GiB with an
/// additional 30 GiB of swap usaged, this will return 125.0. Assumes the provided &System has already been refreshed
/// for memory and swap.
pub fn get_memory_and_swap_usage(system: &System) -> f32 {
    let memory_usage_mib =
        system.used_memory() / (1024 * 1024) + system.used_swap() / (1024 * 1024);
    let memory_total_mib = system.total_memory() / (1024 * 1024);

    memory_usage_mib as f32 / memory_total_mib as f32 * 100.0
}
