use std::fs::{self, Permissions};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use tracing::{debug, info};

use super::error::BaseImageError;

const FCNET_SETUP_SCRIPT: &str = r#"#!/usr/bin/env bash

main() {
    ip addr add 192.168.1.2/30 dev eth0
    ip addr add fd00:fe11:deed:1337::2/126 dev eth0

    ip link set eth0 up

    ip route add default via 192.168.1.1 dev eth0
    ip route add default via fd00:fe11:deed:1337::1 dev eth0
}
main
"#;

const FCNET_SERVICE: &str = r#"[Service]
Type=oneshot
ExecStart=/usr/local/bin/fcnet-setup.sh
[Install]
WantedBy=sshd.service
"#;

/// Kept in sync with scripts/base-image/configure-image.sh
const NOTIFY_READY_SCRIPT: &str = r#"#!/bin/sh
# This script reads chelsea_notify_boot_url_template from /proc/cmdline, which should be a URL
# containing the template string ":vm_id" (without quotes).
# ":vm_id" will be substituted out for the VM ID (from cmdline or /etc/vm_id)
set -eu

# Extract chelsea_vm_id from /proc/cmdline and write to /etc/vm_id if not present
if [ ! -f /etc/vm_id ]; then
    chelsea_vm_id=$(cat /proc/cmdline | grep -oE 'chelsea_vm_id=[^ ]+' | cut -d= -f2- || true)
    if [ -n "${chelsea_vm_id}" ]; then
        echo "${chelsea_vm_id}" > /etc/vm_id
        echo "Wrote VM ID to /etc/vm_id from kernel cmdline"
    else
        echo "chelsea_vm_id not found in /proc/cmdline and /etc/vm_id does not exist" >&2
        exit 1
    fi
fi

# Extract chelsea_notify_boot_url_template from /proc/cmdline
chelsea_notify_boot_url_template=$(cat /proc/cmdline | grep -oE 'chelsea_notify_boot_url_template=[^ ]+' | cut -d= -f2- || true)

if [ -z "${chelsea_notify_boot_url_template}" ]; then
    echo "chelsea_notify_boot_url_template not found in /proc/cmdline" >&2
    exit 1
fi

vm_id=$(cat /etc/vm_id)
url=$(echo "$chelsea_notify_boot_url_template" | sed "s/:vm_id/$vm_id/g")

echo "Sending ready notification to URL $url"
/usr/bin/curl -X POST -H "Content-Type: application/json" -d '{"tag_name" : "ready", "tag_value" : "true"}' "${url}"
"#;

const NOTIFY_READY_SERVICE: &str = r#"[Unit]
Description=Machine Readiness Notification
Wants=fcnet.service
After=fcnet.service
[Service]
Type=oneshot
ExecStart=/usr/local/bin/notify-ready.sh
"#;

/// Systemd service for the Chelsea in-VM management agent.
/// Listens on vsock for host commands (exec, file transfer, SSH key install, etc.).
/// The agent sends a Ready event over vsock on startup, which is the primary
/// readiness signal for new VMs. The legacy notify-ready.service is kept for
/// backward compatibility with older VM snapshots.
const CHELSEA_AGENT_SERVICE: &str = r#"[Unit]
Description=Chelsea In-VM Management Agent
After=network.target
[Service]
Type=simple
ExecStart=/usr/local/bin/chelsea-agent
Restart=on-failure
RestartSec=1
[Install]
WantedBy=multi-user.target
"#;

/// Kept in sync with scripts/base-image/configure-image.sh
const SSH_SETUP_SCRIPT: &str = r#"#!/bin/sh
# This script reads chelsea_ssh_pubkey from /proc/cmdline and sets up authorized_keys.
# The pubkey is expected to be base64-encoded (the middle part of an openssh public key).
set -eu

# Check if authorized_keys already exists with content (e.g., from commit restore)
if [ -f /root/.ssh/authorized_keys ] && [ -s /root/.ssh/authorized_keys ]; then
    echo "SSH authorized_keys already configured, skipping"
    exit 0
fi

# Extract chelsea_ssh_pubkey from /proc/cmdline
chelsea_ssh_pubkey=$(cat /proc/cmdline | grep -oE 'chelsea_ssh_pubkey=[^ ]+' | cut -d= -f2- || true)

if [ -z "${chelsea_ssh_pubkey}" ]; then
    echo "chelsea_ssh_pubkey not found in /proc/cmdline" >&2
    exit 1
fi

# Set up .ssh directory
mkdir -p /root/.ssh
chmod 700 /root/.ssh

# Write the public key in openssh format (assuming ed25519)
echo "ssh-ed25519 ${chelsea_ssh_pubkey}" > /root/.ssh/authorized_keys
chmod 600 /root/.ssh/authorized_keys

echo "SSH authorized_keys configured from kernel cmdline"
"#;

/// Kept in sync with scripts/base-image/configure-image.sh
const SSH_SETUP_SERVICE: &str = r#"[Unit]
Description=Configure SSH keys from kernel cmdline
Before=sshd.service
[Service]
Type=oneshot
ExecStart=/usr/local/bin/ssh-setup.sh
RemainAfterExit=yes
"#;

const DEFAULT_HOSTNAME: &str = "hdr\n";

/// Injects Chelsea's network setup scripts and systemd services into a filesystem root.
///
/// If `agent_binary_path` is provided, the chelsea-agent binary is copied into
/// the rootfs and a systemd service is enabled to start it at boot. The agent
/// provides vsock-based management (exec, file transfer, SSH keys, etc.) and
/// sends a `Ready` event on startup.
///
/// The legacy `notify-ready` service is always installed for backward compatibility
/// with older VM snapshots that don't have the agent.
pub fn configure_filesystem(
    fs_root: &Path,
    agent_binary_path: Option<&Path>,
) -> Result<(), BaseImageError> {
    info!(?fs_root, "Configuring filesystem with Chelsea scripts");

    // Create required directories
    create_dir_if_not_exists(&fs_root.join("usr/local/bin"))?;
    create_dir_if_not_exists(&fs_root.join("etc/systemd/system"))?;
    create_dir_if_not_exists(&fs_root.join("etc/systemd/system/sysinit.target.wants"))?;

    // Write fcnet-setup.sh
    let fcnet_script_path = fs_root.join("usr/local/bin/fcnet-setup.sh");
    write_executable_script(&fcnet_script_path, FCNET_SETUP_SCRIPT)?;

    // Write fcnet.service
    let fcnet_service_path = fs_root.join("etc/systemd/system/fcnet.service");
    write_file(&fcnet_service_path, FCNET_SERVICE)?;

    // Write notify-ready.sh (legacy — kept for backward compat with older snapshots)
    let notify_script_path = fs_root.join("usr/local/bin/notify-ready.sh");
    write_executable_script(&notify_script_path, NOTIFY_READY_SCRIPT)?;

    // Write notify-ready.service (legacy)
    let notify_service_path = fs_root.join("etc/systemd/system/notify-ready.service");
    write_file(&notify_service_path, NOTIFY_READY_SERVICE)?;

    // Write ssh-setup.sh
    let ssh_setup_script_path = fs_root.join("usr/local/bin/ssh-setup.sh");
    write_executable_script(&ssh_setup_script_path, SSH_SETUP_SCRIPT)?;

    // Write ssh-setup.service
    let ssh_setup_service_path = fs_root.join("etc/systemd/system/ssh-setup.service");
    write_file(&ssh_setup_service_path, SSH_SETUP_SERVICE)?;

    // Install chelsea-agent if binary is provided
    if let Some(agent_path) = agent_binary_path {
        install_chelsea_agent(fs_root, agent_path)?;
    } else {
        info!("No chelsea-agent binary provided, skipping agent installation");
    }

    // Create symlinks to enable services at boot
    let sysinit_wants = fs_root.join("etc/systemd/system/sysinit.target.wants");

    create_symlink_if_not_exists(
        Path::new("/etc/systemd/system/fcnet.service"),
        &sysinit_wants.join("fcnet.service"),
    )?;

    create_symlink_if_not_exists(
        Path::new("/etc/systemd/system/notify-ready.service"),
        &sysinit_wants.join("notify-ready.service"),
    )?;

    create_symlink_if_not_exists(
        Path::new("/etc/systemd/system/ssh-setup.service"),
        &sysinit_wants.join("ssh-setup.service"),
    )?;

    // Configure hostname
    let hostname_path = fs_root.join("etc/hostname");
    write_file(&hostname_path, DEFAULT_HOSTNAME)?;

    // Configure DNS resolvers
    let resolv_path = fs_root.join("etc/resolv.conf");
    write_file(&resolv_path, "nameserver 1.1.1.1\nnameserver 8.8.8.8\n")?;

    info!(?fs_root, "Filesystem configuration complete");
    Ok(())
}

/// Copies the chelsea-agent binary into the rootfs and enables its systemd service.
fn install_chelsea_agent(fs_root: &Path, agent_binary_path: &Path) -> Result<(), BaseImageError> {
    info!(?agent_binary_path, "Installing chelsea-agent into rootfs");

    if !agent_binary_path.exists() {
        return Err(BaseImageError::Other(format!(
            "Chelsea agent binary not found at: {}",
            agent_binary_path.display()
        )));
    }

    // Copy binary
    let dest = fs_root.join("usr/local/bin/chelsea-agent");
    fs::copy(agent_binary_path, &dest).map_err(|e| {
        BaseImageError::CopyFiles(format!(
            "Failed to copy chelsea-agent binary to {}: {}",
            dest.display(),
            e
        ))
    })?;

    // Ensure executable
    fs::set_permissions(&dest, Permissions::from_mode(0o755)).map_err(|e| {
        BaseImageError::SetPermissions {
            path: dest.clone(),
            source: e,
        }
    })?;

    // Write systemd service
    let service_path = fs_root.join("etc/systemd/system/chelsea-agent.service");
    write_file(&service_path, CHELSEA_AGENT_SERVICE)?;

    // Enable at boot via multi-user.target.wants symlink
    let multi_user_wants = fs_root.join("etc/systemd/system/multi-user.target.wants");
    create_dir_if_not_exists(&multi_user_wants)?;
    create_symlink_if_not_exists(
        Path::new("/etc/systemd/system/chelsea-agent.service"),
        &multi_user_wants.join("chelsea-agent.service"),
    )?;

    info!("Chelsea agent installed and enabled");
    Ok(())
}

fn create_dir_if_not_exists(path: &Path) -> Result<(), BaseImageError> {
    if !path.exists() {
        debug!(?path, "Creating directory");
        fs::create_dir_all(path).map_err(|e| BaseImageError::CreateDirectory {
            path: path.to_path_buf(),
            source: e,
        })?;
    }
    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<(), BaseImageError> {
    debug!(?path, "Writing file");
    fs::write(path, content).map_err(|e| BaseImageError::WriteFile {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

fn write_executable_script(path: &Path, content: &str) -> Result<(), BaseImageError> {
    write_file(path, content)?;

    debug!(?path, "Setting executable permissions");
    fs::set_permissions(path, Permissions::from_mode(0o755)).map_err(|e| {
        BaseImageError::SetPermissions {
            path: path.to_path_buf(),
            source: e,
        }
    })?;

    Ok(())
}

fn create_symlink_if_not_exists(target: &Path, link_path: &Path) -> Result<(), BaseImageError> {
    if link_path.exists() || link_path.is_symlink() {
        debug!(?link_path, "Symlink already exists, skipping");
        return Ok(());
    }

    debug!(?target, ?link_path, "Creating symlink");
    if let Err(e) = symlink(target, link_path) {
        // Ignore "already exists" errors (race condition is acceptable)
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            debug!(?link_path, "Symlink already exists, continuing");
            return Ok(());
        }
        return Err(BaseImageError::CreateSymlink {
            from: target.to_path_buf(),
            to: link_path.to_path_buf(),
            source: e,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_configure_filesystem_without_agent() {
        let temp_dir = TempDir::new().unwrap();
        let fs_root = temp_dir.path();

        configure_filesystem(fs_root, None).unwrap();

        // Verify key files were created
        assert!(fs_root.join("usr/local/bin/fcnet-setup.sh").exists());
        assert!(fs_root.join("usr/local/bin/notify-ready.sh").exists());
        assert!(fs_root.join("etc/systemd/system/fcnet.service").exists());
        assert!(
            fs_root
                .join("etc/systemd/system/notify-ready.service")
                .exists()
        );
        assert!(fs_root.join("etc/hostname").exists());

        // Verify symlinks
        assert!(
            fs_root
                .join("etc/systemd/system/sysinit.target.wants/fcnet.service")
                .is_symlink()
        );
        assert!(
            fs_root
                .join("etc/systemd/system/sysinit.target.wants/notify-ready.service")
                .is_symlink()
        );

        // Agent should NOT be present
        assert!(!fs_root.join("usr/local/bin/chelsea-agent").exists());
        assert!(
            !fs_root
                .join("etc/systemd/system/chelsea-agent.service")
                .exists()
        );

        // Verify hostname
        let hostname = fs::read_to_string(fs_root.join("etc/hostname")).unwrap();
        assert_eq!(hostname, "hdr\n");

        // Verify scripts are executable
        let mode = fs::metadata(fs_root.join("usr/local/bin/fcnet-setup.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn test_configure_filesystem_with_agent() {
        let temp_dir = TempDir::new().unwrap();
        let fs_root = temp_dir.path();

        // Create a fake agent binary
        let agent_dir = TempDir::new().unwrap();
        let agent_path = agent_dir.path().join("chelsea-agent");
        fs::write(&agent_path, b"#!/bin/sh\necho fake agent").unwrap();

        configure_filesystem(fs_root, Some(&agent_path)).unwrap();

        // Legacy services still present
        assert!(fs_root.join("usr/local/bin/notify-ready.sh").exists());
        assert!(
            fs_root
                .join("etc/systemd/system/notify-ready.service")
                .exists()
        );
        assert!(
            fs_root
                .join("etc/systemd/system/sysinit.target.wants/notify-ready.service")
                .is_symlink()
        );

        // Agent binary installed
        assert!(fs_root.join("usr/local/bin/chelsea-agent").exists());
        let mode = fs::metadata(fs_root.join("usr/local/bin/chelsea-agent"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);

        // Agent content matches
        let content = fs::read(fs_root.join("usr/local/bin/chelsea-agent")).unwrap();
        assert_eq!(content, b"#!/bin/sh\necho fake agent");

        // Agent service installed and enabled
        assert!(
            fs_root
                .join("etc/systemd/system/chelsea-agent.service")
                .exists()
        );
        assert!(
            fs_root
                .join("etc/systemd/system/multi-user.target.wants/chelsea-agent.service")
                .is_symlink()
        );
    }

    #[test]
    fn test_configure_filesystem_agent_binary_missing() {
        let temp_dir = TempDir::new().unwrap();
        let fs_root = temp_dir.path();

        let result = configure_filesystem(fs_root, Some(Path::new("/nonexistent/chelsea-agent")));
        assert!(result.is_err());
    }

    #[test]
    fn test_configure_filesystem_idempotent_without_agent() {
        let temp_dir = TempDir::new().unwrap();
        let fs_root = temp_dir.path();

        configure_filesystem(fs_root, None).unwrap();
        configure_filesystem(fs_root, None).unwrap();

        assert!(fs_root.join("usr/local/bin/fcnet-setup.sh").exists());
    }

    #[test]
    fn test_configure_filesystem_idempotent_with_agent() {
        let temp_dir = TempDir::new().unwrap();
        let fs_root = temp_dir.path();

        let agent_dir = TempDir::new().unwrap();
        let agent_path = agent_dir.path().join("chelsea-agent");
        fs::write(&agent_path, b"#!/bin/sh\necho fake agent").unwrap();

        configure_filesystem(fs_root, Some(&agent_path)).unwrap();
        configure_filesystem(fs_root, Some(&agent_path)).unwrap();

        // Binary still present and correct
        assert!(fs_root.join("usr/local/bin/chelsea-agent").exists());
        let content = fs::read(fs_root.join("usr/local/bin/chelsea-agent")).unwrap();
        assert_eq!(content, b"#!/bin/sh\necho fake agent");

        // Service and symlink still correct
        assert!(
            fs_root
                .join("etc/systemd/system/chelsea-agent.service")
                .exists()
        );
        assert!(
            fs_root
                .join("etc/systemd/system/multi-user.target.wants/chelsea-agent.service")
                .is_symlink()
        );
    }
}
