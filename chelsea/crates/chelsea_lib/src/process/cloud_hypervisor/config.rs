use std::{fmt::Display, path::PathBuf, sync::Arc};

use macaddr::MacAddr6;
use ssh_key::PublicKey;
use uuid::Uuid;

use crate::{network::VmNetwork, vm::VmConfig, volume::VmVolume};

/// Working directory root for cloud-hypervisor VMs
/// (Note: Not a chroot jail - ch-jailer not used)
const JAIL_DIR_ROOT: &str = "/srv/jailer/cloud-hypervisor";

/// Returns the working directory for a given VM ID
pub fn get_jail_root_by_vm_id(vm_id: &Uuid) -> PathBuf {
    PathBuf::from(JAIL_DIR_ROOT).join(vm_id.to_string())
}

/// Represents a path in the VM's working directory
/// (Note: Not actually "jailed" - just organized per-VM)
#[derive(Clone, Debug)]
pub struct PathBufJailer {
    vm_id: Uuid,
    inner: PathBuf,
}

impl PathBufJailer {
    /// Constructs a new PathBufJailer
    pub fn new(vm_id: Uuid, inner: PathBuf) -> Self {
        Self { vm_id, inner }
    }

    /// The fully-qualified path from the perspective of the host
    pub fn with_jail_root(&self) -> PathBuf {
        // If inner is already absolute, just return it
        if self.inner.is_absolute() {
            return self.inner.clone();
        }

        // Otherwise, make it relative to VM working directory
        let mut base = get_jail_root_by_vm_id(&self.vm_id);
        base.push(&self.inner);
        base
    }

    /// Returns the path (kept for compatibility)
    pub fn without_jail_root(&self) -> PathBuf {
        self.inner.clone()
    }

    pub fn file_name<'a>(&'a self) -> Option<&'a str> {
        self.inner
            .file_name()
            .and_then(|file_name| file_name.to_str())
    }
}

/// The necessary config for a CloudHypervisorProcess
pub struct CloudHypervisorProcessConfig {
    pub vm_id: Uuid,
    pub api_socket: PathBufJailer,
    pub kernel: CloudHypervisorProcessKernelConfig,
    pub disk: CloudHypervisorProcessDiskConfig,
    pub network: CloudHypervisorProcessNetworkConfig,
    pub cpus: CloudHypervisorProcessCpuConfig,
    pub memory: CloudHypervisorProcessMemoryConfig,
    pub log_file: PathBufJailer,
}

impl CloudHypervisorProcessConfig {
    /// A convenience method for constructing the config object using values derived from other structs
    pub async fn with_defaults(
        vm_id: Uuid,
        volume: &Arc<dyn VmVolume>,
        network: &VmNetwork,
        vm_config: &VmConfig,
        chelsea_notify_boot_url_template: impl Display,
    ) -> Self {
        // Extract base64 part of SSH public key for kernel cmdline
        // Format is: "ssh-ed25519 AAAAC3NzaC1..." - we want just the base64 part
        let public_key = PublicKey::from(vm_config.ssh_keypair.public.clone());
        let openssh_key = public_key.to_openssh().unwrap_or_default();
        let ssh_pubkey_b64 = openssh_key.split_whitespace().nth(1).unwrap_or("");

        CloudHypervisorProcessConfig {
            vm_id,
            api_socket: PathBufJailer::new(vm_id, PathBuf::from("/run/ch.sock")),
            kernel: CloudHypervisorProcessKernelConfig {
                path: PathBufJailer::new(
                    vm_id,
                    PathBuf::from("kernels").join(&vm_config.kernel_name), // Jail-relative path
                ),
                cmdline: format!(
                    "console=ttyS0 reboot=k panic=1 root=/dev/vda rw net.ifnames=0 chelsea_entropy={} chelsea_vm_id={} chelsea_ssh_pubkey={} chelsea_notify_boot_url_template={chelsea_notify_boot_url_template} nohz=off",
                    Uuid::new_v4().as_u128(),
                    vm_id,
                    ssh_pubkey_b64
                ),
            },
            disk: CloudHypervisorProcessDiskConfig {
                path: volume.path(),
                readonly: false,
            },
            network: CloudHypervisorProcessNetworkConfig {
                tap: network.tap_name(),
                mac: network.guest_mac(),
            },
            cpus: CloudHypervisorProcessCpuConfig {
                boot_vcpus: vm_config.vcpu_count,
            },
            memory: CloudHypervisorProcessMemoryConfig {
                size_mib: vm_config.mem_size_mib,
            },
            log_file: PathBufJailer::new(vm_id, PathBuf::from("/cloud-hypervisor.log")),
        }
    }
}

pub struct CloudHypervisorProcessKernelConfig {
    pub path: PathBufJailer,
    pub cmdline: String,
}

pub struct CloudHypervisorProcessDiskConfig {
    pub path: PathBuf,
    pub readonly: bool,
}

pub struct CloudHypervisorProcessNetworkConfig {
    pub tap: String,
    pub mac: MacAddr6,
}

pub struct CloudHypervisorProcessCpuConfig {
    pub boot_vcpus: u32,
}

pub struct CloudHypervisorProcessMemoryConfig {
    pub size_mib: u32,
}

/// Configuration for the vsock device, enabling host-to-guest communication
/// without network overhead.
pub struct CloudHypervisorProcessVsockConfig {
    /// The guest Context ID. Standard value is 3 for guests.
    pub guest_cid: u64,
    /// The path to the Unix domain socket that cloud-hypervisor will create
    /// for host-to-guest vsock communication (VM working directory-relative).
    pub socket_path: PathBufJailer,
}

impl CloudHypervisorProcessVsockConfig {
    /// Creates a new vsock configuration with standard defaults.
    /// - guest_cid: 3 (standard guest CID)
    /// - socket_path: run/vsock.sock (inside VM working directory)
    pub fn with_defaults(vm_id: Uuid) -> Self {
        Self {
            guest_cid: 3,
            socket_path: PathBufJailer::new(vm_id, PathBuf::from("run/vsock.sock")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jail_root_by_vm_id() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let jail_root = get_jail_root_by_vm_id(&vm_id);

        assert_eq!(
            jail_root,
            PathBuf::from("/srv/jailer/cloud-hypervisor/550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn test_pathbuf_jailer_with_jail_root() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        // Use a relative path so it gets jail-rooted
        let path = PathBufJailer::new(vm_id, PathBuf::from("run/ch.sock"));

        let with_root = path.with_jail_root();
        assert_eq!(
            with_root,
            PathBuf::from(
                "/srv/jailer/cloud-hypervisor/550e8400-e29b-41d4-a716-446655440000/run/ch.sock"
            )
        );
    }

    #[test]
    fn test_pathbuf_jailer_without_jail_root() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let path = PathBufJailer::new(vm_id, PathBuf::from("/run/ch.sock"));

        let without_root = path.without_jail_root();
        assert_eq!(without_root, PathBuf::from("/run/ch.sock"));
    }

    #[test]
    fn test_pathbuf_jailer_absolute_path_handling() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        // Absolute path should have leading slash stripped when appending to jail root
        let path = PathBufJailer::new(vm_id, PathBuf::from("/absolute/path"));

        let with_root = path.with_jail_root();
        assert!(with_root.to_string_lossy().ends_with("/absolute/path"));
        assert!(!with_root.to_string_lossy().contains("//"));
    }

    #[test]
    fn test_pathbuf_jailer_file_name() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let path = PathBufJailer::new(vm_id, PathBuf::from("/path/to/file.txt"));

        assert_eq!(path.file_name(), Some("file.txt"));
    }

    #[test]
    fn test_cloud_hypervisor_process_config_defaults() {
        let vm_id = Uuid::new_v4();

        // Test that config fields use proper jail paths
        let log_path = PathBufJailer::new(vm_id, PathBuf::from("/cloud-hypervisor.log"));
        assert_eq!(
            log_path.without_jail_root(),
            PathBuf::from("/cloud-hypervisor.log")
        );

        let api_socket = PathBufJailer::new(vm_id, PathBuf::from("/run/ch.sock"));
        assert_eq!(
            api_socket.without_jail_root(),
            PathBuf::from("/run/ch.sock")
        );
    }
}
