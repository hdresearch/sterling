mod cgroup;
mod ensure_loopback;
mod identity;
mod lockfile;
mod orphan_interfaces;
mod provisioning;

pub use cgroup::ensure_vm_cgroup_exists;
pub use ensure_loopback::ensure_ipv4_on_loopback;
pub use identity::get_or_create_identity;
pub use lockfile::create_lockfile;
pub use orphan_interfaces::cleanup_orphaned_wg_interfaces;
pub use provisioning::validate_vm_provisioning_parameters;
