use chelsea_lib::cgroup::{CgroupError, ensure_vm_cgroup};

/// Ensure the VM cgroup is created and configured at startup.
///
/// This creates a cgroup that all VM processes will be placed into,
/// ensuring they can only compete with each other for CPU — not with
/// host processes like chelsea itself.
pub async fn ensure_vm_cgroup_exists(
    cgroup_name: &str,
    cpu_weight: u32,
) -> Result<(), CgroupError> {
    ensure_vm_cgroup(cgroup_name, cpu_weight).await?;
    Ok(())
}
