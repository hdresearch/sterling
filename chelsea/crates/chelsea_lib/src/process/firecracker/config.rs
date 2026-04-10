use std::{fmt::Display, path::PathBuf, sync::Arc};

use macaddr::MacAddr6;
use ssh_key::PublicKey;
use uuid::Uuid;

use crate::{
    data_dir::DataDir,
    network::VmNetwork,
    process::firecracker::{constants::default_log_path, types::MachineConfiguration},
    vm::VmConfig,
    volume::VmVolume,
};

/// DO NOT MODIFY. This is a hardcoded assumption of jailer
const JAIL_DIR_ROOT: &str = "/srv/jailer/firecracker";

/// Returns the chroot dir for a given Vm ID
pub fn get_jail_root_by_vm_id(vm_id: &Uuid) -> PathBuf {
    PathBuf::from(JAIL_DIR_ROOT)
        .join(vm_id.to_string())
        .join("root")
}

/// Represents a chrooted path for a firecracker jail; resolves to /srv/jailer/firecracker/{vm_id}/root
#[derive(Clone, Debug)]
pub struct PathBufJailer {
    vm_id: Uuid,
    inner: PathBuf,
}

impl PathBufJailer {
    /// Constructs a new PathBufJailer, which wraps a route in a layer that can optionally append a jail root dir for files which may be referred to from the Firecracker API.
    pub fn new(vm_id: Uuid, inner: PathBuf) -> Self {
        Self { vm_id, inner }
    }

    /// The fully-qualified path from the perspective of the host (includes jail root)
    pub fn with_jail_root(&self) -> PathBuf {
        let mut base = get_jail_root_by_vm_id(&self.vm_id);
        // If inner is absolute, append its components (don't replace base)
        let inner_path = if self.inner.is_absolute() {
            self.inner.strip_prefix("/").unwrap_or(&self.inner)
        } else {
            self.inner.as_path()
        };
        base.push(inner_path);
        base
    }

    /// The jail-relative path from the perspective of a chrooted process (does not include jail root)
    pub fn without_jail_root(&self) -> PathBuf {
        self.inner.clone()
    }

    pub fn file_name<'a>(&'a self) -> Option<&'a str> {
        self.inner
            .file_name()
            .and_then(|file_name| file_name.to_str())
    }
}

/// The necessary config for a FirecrackerProcess. Note that because Firecracker is spawned from jailer, all "host" paths will have to be relative to the jail dir.
pub struct FirecrackerProcessConfig {
    pub boot_source: FirecrackerProcessBootSourceConfig,
    pub drive: FirecrackerProcessDriveConfig,
    /// Deprecated; unused
    pub logger: FirecrackerProcessLoggerConfig,
    pub machine: FirecrackerProcessMachineConfig,
    pub network: FirecrackerProcessNetworkConfig,
}

impl FirecrackerProcessConfig {
    /// A convenience method for constructing the config object using values derived from other structs which have to be created during VM spawning.
    pub fn with_defaults(
        vm_id: Uuid,
        volume: &Arc<dyn VmVolume>,
        network: &VmNetwork,
        vm_config: &VmConfig,
        chelsea_notify_boot_url_template: impl Display,
    ) -> Self {
        let data_dir = DataDir::global();
        FirecrackerProcessConfig {
            boot_source: FirecrackerProcessBootSourceConfig {
                kernel_path: PathBufJailer::new(
                    vm_id.clone(),
                    data_dir.kernel_dir.join(&vm_config.kernel_name),
                ),
                boot_args: {
                    // Extract the base64-encoded SSH public key from the openssh format
                    // Format is: "ssh-ed25519 AAAAC3NzaC1..." - we want just the base64 part
                    let public_key = PublicKey::from(vm_config.ssh_keypair.public.clone());
                    let openssh_key = public_key.to_openssh().unwrap_or_default();
                    let ssh_pubkey_b64 = openssh_key.split_whitespace().nth(1).unwrap_or("");

                    format!(
                        "console=ttyS0 reboot=k panic=1 pci=off quiet loglevel=1 nomodule \
                         8250.nr_uarts=0 i8042.noaux i8042.nomux i8042.dumbkbd swiotlb=noforce \
                         chelsea_entropy={} chelsea_vm_id={} chelsea_ssh_pubkey={} \
                         chelsea_notify_boot_url_template={chelsea_notify_boot_url_template}",
                        Uuid::new_v4().as_u128(),
                        vm_id,
                        ssh_pubkey_b64
                    )
                },
            },
            drive: FirecrackerProcessDriveConfig {
                drive_id: "root".to_string(),
                path_on_host: volume.path(),
                is_root_device: true,
                is_read_only: false,
            },
            logger: FirecrackerProcessLoggerConfig::with_defaults(vm_id),
            machine: FirecrackerProcessMachineConfig {
                mem_size_mib: vm_config.mem_size_mib,
                vcpu_count: vm_config.vcpu_count,
                ..Default::default()
            },
            network: FirecrackerProcessNetworkConfig {
                guest_mac: network.guest_mac(),
                host_dev_name: network.tap_name(),
                iface_id: "net0".to_string(),
            },
        }
    }
}

pub struct FirecrackerProcessBootSourceConfig {
    /// The path to the kernel
    pub kernel_path: PathBufJailer,
    /// The kernel boot args
    pub boot_args: String,
}

pub struct FirecrackerProcessDriveConfig {
    /// The drive ID, eg: root
    pub drive_id: String,
    /// The path to the drive; for the path to a drive from the VM's perspective, see VersConfig::vm_root_drive_path.
    pub path_on_host: PathBuf,
    /// Whether or not the drive is root. In our case, probably true for now; we only set up one drive - the boot drive
    pub is_root_device: bool,
    /// Whether the drive is read only. Almost certainly false here.
    pub is_read_only: bool,
}

pub struct FirecrackerProcessNetworkConfig {
    /// The guest MAC address for the VM; as per the Firecracker defaults, this will be used to set the guest IP according to the format 06:00:xx:xx:xx:xx
    pub guest_mac: MacAddr6,
    /// The device that will deliver ethernet frames to the VM, eg: a TAP device
    pub host_dev_name: String,
    /// The network interface ID, default: net1
    pub iface_id: String,
}

pub enum FirecrackerProcessLoggerLogLevel {
    Error,
    Warning,
    Info,
    Debug,
    Trace,
    Off,
}

impl ToString for FirecrackerProcessLoggerLogLevel {
    fn to_string(&self) -> String {
        match self {
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Trace => "Trace",
            Self::Off => "Off",
        }
        .to_string()
    }
}

pub struct FirecrackerProcessLoggerConfig {
    /// The path that Firecracker will log to
    pub log_path: PathBufJailer,
    /// The log verbosity level
    pub level: FirecrackerProcessLoggerLogLevel,
    /// Whether or not the log should include the level
    pub show_level: bool,
    pub show_origin: bool,
}

pub type FirecrackerProcessMachineConfig = MachineConfiguration;

impl FirecrackerProcessLoggerConfig {
    pub fn with_defaults(vm_id: Uuid) -> Self {
        Self {
            log_path: default_log_path(vm_id),
            level: FirecrackerProcessLoggerLogLevel::Debug,
            show_level: true,
            show_origin: true,
        }
    }
}

/// Configuration for the vsock device, enabling host-to-guest communication
/// without network overhead.
pub struct FirecrackerProcessVsockConfig {
    /// The vsock device identifier (e.g., "vsock0").
    pub vsock_id: String,
    /// The guest Context ID. Standard value is 3 for guests.
    pub guest_cid: u64,
    /// The path to the Unix domain socket that Firecracker will create
    /// for host-to-guest vsock communication (jail-relative).
    pub uds_path: PathBufJailer,
}

impl FirecrackerProcessVsockConfig {
    /// Creates a new vsock configuration with standard defaults.
    /// - vsock_id: "vsock0"
    /// - guest_cid: 3 (standard guest CID)
    /// - uds_path: /run/vsock.sock (inside jail)
    pub fn with_defaults(vm_id: Uuid) -> Self {
        Self {
            vsock_id: "vsock0".to_string(),
            guest_cid: 3,
            uds_path: PathBufJailer::new(vm_id, PathBuf::from("/run/vsock.sock")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vsock_config_defaults() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let config = FirecrackerProcessVsockConfig::with_defaults(vm_id);

        assert_eq!(config.vsock_id, "vsock0");
        assert_eq!(config.guest_cid, 3);
        // The jail-relative path should end with /run/vsock.sock
        let host_path = config.uds_path.with_jail_root();
        assert!(
            host_path.ends_with("run/vsock.sock"),
            "expected path to end with run/vsock.sock, got: {}",
            host_path.display()
        );
    }

    #[test]
    fn test_vsock_config_jail_path_contains_vm_id() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let config = FirecrackerProcessVsockConfig::with_defaults(vm_id);

        let host_path = config.uds_path.with_jail_root();
        assert!(
            host_path.to_string_lossy().contains(&vm_id.to_string()),
            "jail path should contain VM ID, got: {}",
            host_path.display()
        );
    }
}
