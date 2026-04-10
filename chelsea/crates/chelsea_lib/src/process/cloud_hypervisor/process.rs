use std::{io, path::PathBuf, process::Stdio, time::Duration};

use anyhow::Context;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use thiserror::Error;
use tokio::process::Command;
use util::defer::DeferAsync;
use uuid::Uuid;
use vers_config::VersConfig;

use crate::util::vm_user::{ChownVmError, get_or_create_vm_user};
use util::linux::UserError;

use super::api::CloudHypervisorApi;
use super::config::get_jail_root_by_vm_id;
use super::error::CloudHypervisorApiError;
use super::types::{CloudHypervisorVmState, VmSnapshotConfig};

const PID_FILE_NAME: &str = "cloud-hypervisor.pid";

#[derive(Debug, Error)]
pub enum CloudHypervisorProcessError {
    #[error("Cloud Hypervisor API error: {0}")]
    Api(#[from] CloudHypervisorApiError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Process error: {0}")]
    Process(String),
    #[error("Timed out waiting for Cloud Hypervisor API socket")]
    ApiSocketTimeout,
    #[error("ch-jailer error: {0}")]
    JailerProcess(io::Error),
    #[error("user error: {0}")]
    User(#[from] UserError),
    #[error("chown error: {0}")]
    ChownError(#[from] ChownVmError),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("expected {0} to be valid pid (u32).")]
    PidParsing(String),
}

/// Represents a running Cloud Hypervisor process (spawned via ch-jailer)
#[derive(Debug)]
pub struct CloudHypervisorProcess {
    /// Cloud Hypervisor API client
    pub api: CloudHypervisorApi,
    /// PathBuf containing the fully-qualified path to the chroot dir
    pub jail_root: PathBuf,
    /// The VM ID
    pub vm_id: Uuid,
}

impl CloudHypervisorProcess {
    /// Creates and starts a new Cloud Hypervisor process
    /// Note: Runs cloud-hypervisor directly (ch-jailer not required)
    /// VM configuration (kernel, disks, etc.) is done via API after spawning
    pub async fn new(
        vm_id: Uuid,
        stdout: impl Into<Stdio>,
        stderr: impl Into<Stdio>,
        netns_name: &str,
    ) -> Result<Self, CloudHypervisorProcessError> {
        let cloud_hypervisor_bin_path = VersConfig::chelsea()
            .cloud_hypervisor_bin_path
            .clone()
            .expect("config 'cloud_hypervisor_bin_path' is required");

        // Check if binary exists before trying to spawn
        if !cloud_hypervisor_bin_path.exists() {
            return Err(CloudHypervisorProcessError::Process(format!(
                "cloud-hypervisor binary not found at {:?}",
                cloud_hypervisor_bin_path
            )));
        }

        let _user_info = get_or_create_vm_user()?;
        let api = CloudHypervisorApi::new(vm_id)?;
        let jail_root = get_jail_root_by_vm_id(&vm_id);

        // Create jail directory structure
        tokio::fs::create_dir_all(&jail_root).await.map_err(|e| {
            CloudHypervisorProcessError::Process(format!(
                "Failed to create jail directory {:?}: {}",
                jail_root, e
            ))
        })?;

        // Create run directory for socket
        let run_dir = jail_root.join("run");
        tokio::fs::create_dir_all(&run_dir).await.map_err(|e| {
            CloudHypervisorProcessError::Process(format!(
                "Failed to create run directory {:?}: {}",
                run_dir, e
            ))
        })?;

        // Spawn cloud-hypervisor with minimal flags - will configure VM via API
        // Note: Don't pass --kernel here, as it auto-creates the VM and conflicts with API create_vm call
        // Serial console output is configured via API in the VM config
        // Run inside the network namespace so cloud-hypervisor can access the tap device
        let mut cmd = Command::new("ip");
        cmd.arg("netns")
            .arg("exec")
            .arg(netns_name)
            .arg(&cloud_hypervisor_bin_path)
            .arg("--api-socket")
            .arg(api.socket_path.with_jail_root())
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(false);

        // Create a new session so the process doesn't get signals from parent
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map(|_| ())
                    .map_err(|e| io::Error::from_raw_os_error(e as i32))
            });
        }

        let mut child = cmd.spawn().map_err(|e| {
            CloudHypervisorProcessError::Process(format!(
                "Failed to spawn cloud-hypervisor from {:?}: {}",
                cloud_hypervisor_bin_path, e
            ))
        })?;

        // Get the PID immediately after spawn
        let pid = child.id().ok_or_else(|| {
            CloudHypervisorProcessError::Process("Failed to get process ID".to_string())
        })?;
        let nix_pid = Pid::from_raw(pid as i32);

        // Set up deferred cleanup - kills process and removes jail dir on failure
        let mut defer = DeferAsync::new();
        defer.defer({
            let jail_root = jail_root.clone();
            async move {
                // Kill the process
                if let Err(e) = signal::kill(nix_pid, Signal::SIGKILL) {
                    if e != nix::errno::Errno::ESRCH {
                        tracing::warn!(pid = pid, error = %e, "Failed to kill cloud-hypervisor process during cleanup");
                    }
                }
                // Remove jail directory
                if let Err(e) = tokio::fs::remove_dir_all(&jail_root).await {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        tracing::warn!(jail_root = %jail_root.display(), error = %e, "Failed to remove jail directory during cleanup");
                    }
                }
            }
        });

        // Write PID to file
        let pid_file = jail_root.join("cloud-hypervisor.pid");
        tokio::fs::write(&pid_file, pid.to_string())
            .await
            .map_err(CloudHypervisorProcessError::Io)?;

        // Wait for the socket file to become available
        let socket_path = api.socket_path.with_jail_root();
        let start = std::time::Instant::now();
        loop {
            if socket_path.exists() {
                break;
            } else if start.elapsed()
                >= Duration::from_secs(VersConfig::chelsea().firecracker_socket_timeout_secs)
            {
                // Try to get exit status for debugging before defer cleanup runs
                let status_msg = match child.try_wait() {
                    Ok(Some(status)) => format!("Process exited with status: {}", status),
                    Ok(None) => "Process still running".to_string(),
                    Err(e) => format!("Failed to check process status: {}", e),
                };

                // Defer cleanup will run automatically when we return Err
                return Err(CloudHypervisorProcessError::Process(format!(
                    "API socket timeout at {:?}. {}. Binary: {:?}",
                    socket_path, status_msg, cloud_hypervisor_bin_path
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Success - disable cleanup
        defer.commit();

        Ok(Self {
            api,
            jail_root,
            vm_id,
        })
    }

    /// Returns the process ID of the cloud-hypervisor process
    pub async fn pid(&self) -> Result<u32, CloudHypervisorProcessError> {
        let raw_content = tokio::fs::read_to_string(self.jail_root.join(PID_FILE_NAME))
            .await
            .map_err(CloudHypervisorProcessError::Io)?;
        let content = raw_content.trim();

        match content.parse::<u32>() {
            Ok(pid) => Ok(pid),
            Err(_) => Err(CloudHypervisorProcessError::PidParsing(content.to_string())),
        }
    }

    /// Kills the cloud-hypervisor process
    pub async fn kill(&self) -> Result<(), CloudHypervisorProcessError> {
        // Get PID before attempting shutdown (we need it to verify the process has exited)
        let pid = match self.pid().await {
            Ok(pid) => pid,
            Err(e) => {
                // If we can't read the PID file, the process may already be gone
                tracing::debug!(vm_id = %self.vm_id, error = %e, "Could not read PID file; process may already be gone");
                // Still try to clean up the jail directory
                return self.cleanup_jail_directory().await;
            }
        };
        let nix_pid = Pid::from_raw(pid as i32);

        // Shutdown via the API doesn't work, but sending SIGTERM does
        // https://github.com/cloud-hypervisor/cloud-hypervisor/pull/1781
        if let Err(e) = signal::kill(nix_pid, Signal::SIGTERM) {
            // ESRCH means process doesn't exist - that's fine
            if e != nix::errno::Errno::ESRCH {
                tracing::warn!(vm_id = %self.vm_id, error = %e, "SIGTERM failed");
            }
        }

        // Wait for the process to actually exit. This is critical because the
        // kernel won't release resources (block devices, network taps, etc.)
        // until the process is fully gone. Without this wait, subsequent VM
        // creations race against kernel cleanup and fail with EBUSY on the tap device.
        const WAIT_TIMEOUT: Duration = Duration::from_secs(30);
        const POLL_INTERVAL: Duration = Duration::from_millis(100);
        // RBD devices can take a while to release - need longer delay than tap devices.
        // Cloud-hypervisor with virtio-blk may hold kernel references longer after SIGKILL.
        const KERNEL_RELEASE_DELAY: Duration = Duration::from_secs(5);
        let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
        let mut sigkill_sent = false;

        loop {
            match signal::kill(nix_pid, None) {
                Ok(_) => {
                    // Process still exists
                    if tokio::time::Instant::now() >= deadline {
                        tracing::error!(
                            vm_id = %self.vm_id,
                            pid = pid,
                            "Process still alive after {WAIT_TIMEOUT:?}; resources may not be released cleanly"
                        );
                        break;
                    }
                    // After 5 seconds, try sending SIGKILL again in case the first one didn't work
                    if !sigkill_sent
                        && tokio::time::Instant::now() >= (deadline - Duration::from_secs(25))
                    {
                        tracing::warn!(vm_id = %self.vm_id, pid = pid, "Process not responding to shutdown, sending SIGKILL");
                        let _ = signal::kill(nix_pid, Signal::SIGKILL);
                        sigkill_sent = true;
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
                Err(nix::errno::Errno::ESRCH) => {
                    // Process is gone - wait briefly for kernel to release resources
                    // (block devices, network taps, etc.)
                    tracing::debug!(
                        vm_id = %self.vm_id,
                        pid = pid,
                        "Process exited after shutdown, waiting for kernel resource release"
                    );
                    tokio::time::sleep(KERNEL_RELEASE_DELAY).await;
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        vm_id = %self.vm_id,
                        pid = pid,
                        error = %e,
                        "Unexpected error checking if process exited; proceeding with cleanup"
                    );
                    break;
                }
            }
        }

        self.cleanup_jail_directory().await
    }

    /// Helper to clean up the jail directory
    async fn cleanup_jail_directory(&self) -> Result<(), CloudHypervisorProcessError> {
        // Delete the jail root (/srv/jailer/cloud-hypervisor/{vm_id})
        // Note: Unlike Firecracker where jail_root is .../root within the VM dir,
        // CloudHypervisor's jail_root IS the VM-specific directory, so we delete it directly.
        // Ignore errors if directory doesn't exist - it may have already been cleaned up
        if let Err(e) = tokio::fs::remove_dir_all(&self.jail_root).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(CloudHypervisorProcessError::JailerProcess(e));
            }
            tracing::debug!(vm_id = %self.vm_id, "Jail directory already removed");
        }

        Ok(())
    }

    /// Returns the VM ID
    pub fn vm_id(&self) -> Uuid {
        self.vm_id
    }

    /// Checks if the VM is paused by querying VM state via the API
    pub async fn is_paused(&self) -> anyhow::Result<bool> {
        let info = self.api.vm_info().await?;
        Ok(info.state == CloudHypervisorVmState::Paused)
    }

    /// Pauses the VM via the Cloud Hypervisor API
    pub async fn pause(&self) -> anyhow::Result<()> {
        self.api.pause_vm().await?;
        Ok(())
    }

    /// Resumes the VM via the Cloud Hypervisor API
    pub async fn resume(&self) -> anyhow::Result<()> {
        self.api.resume_vm().await?;
        Ok(())
    }

    /// Create a snapshot of the VM. The snapshot is stored as a tar archive
    /// in the snapshot directory. Returns the filename of the archive.
    pub async fn create_snapshot(&self, snapshot_id: &str) -> anyhow::Result<String> {
        let snapshot_dir = &VersConfig::chelsea().snapshot_dir;

        tracing::debug!(
            vm_id = %self.vm_id,
            snapshot_id = %snapshot_id,
            "Creating Cloud Hypervisor snapshot directory"
        );

        // Create a temporary directory inside the jail for CH to write the snapshot to
        let ch_snapshot_dir = self.jail_root.join(format!("snapshot-{}", snapshot_id));
        tokio::fs::create_dir_all(&ch_snapshot_dir)
            .await
            .context("Creating CH snapshot directory")?;

        // Tell cloud-hypervisor to snapshot to the directory
        let destination_url = format!("file://{}", ch_snapshot_dir.display());

        tracing::info!(
            vm_id = %self.vm_id,
            snapshot_id = %snapshot_id,
            destination_url = %destination_url,
            "Calling Cloud Hypervisor API: vm.snapshot"
        );

        self.api
            .snapshot_vm(&VmSnapshotConfig { destination_url })
            .await
            .context("Cloud Hypervisor vm.snapshot API call")?;

        tracing::debug!(
            vm_id = %self.vm_id,
            snapshot_id = %snapshot_id,
            "Cloud Hypervisor vm.snapshot completed, listing snapshot contents"
        );

        // List snapshot directory contents for debugging
        if let Ok(entries) = tokio::fs::read_dir(&ch_snapshot_dir).await {
            let mut entry_stream = entries;
            let mut files = Vec::new();
            while let Ok(Some(entry)) = entry_stream.next_entry().await {
                if let Ok(file_name) = entry.file_name().into_string() {
                    files.push(file_name);
                }
            }
            tracing::debug!(
                vm_id = %self.vm_id,
                snapshot_id = %snapshot_id,
                files = ?files,
                "Cloud Hypervisor snapshot directory contents"
            );
        }

        // Pack the snapshot directory into a tar archive in snapshot_dir
        let archive_name = format!("{}.ch_snapshot.tar", snapshot_id);
        let archive_path = snapshot_dir.join(&archive_name);

        tracing::debug!(
            vm_id = %self.vm_id,
            snapshot_id = %snapshot_id,
            archive_path = %archive_path.display(),
            "Creating tar archive from snapshot directory"
        );

        let status = Command::new("tar")
            .arg("cf")
            .arg(&archive_path)
            .arg("-C")
            .arg(&ch_snapshot_dir)
            .arg(".")
            .status()
            .await
            .context("Running tar to archive CH snapshot")?;

        if !status.success() {
            anyhow::bail!(
                "tar failed to archive CH snapshot (exit code: {:?})",
                status.code()
            );
        }

        // Get archive size for logging
        if let Ok(metadata) = tokio::fs::metadata(&archive_path).await {
            tracing::info!(
                vm_id = %self.vm_id,
                snapshot_id = %snapshot_id,
                archive_name = %archive_name,
                archive_size_bytes = metadata.len(),
                "Cloud Hypervisor snapshot archived successfully"
            );
        }

        // Clean up the temporary snapshot directory
        let _ = tokio::fs::remove_dir_all(&ch_snapshot_dir).await;

        Ok(archive_name)
    }

    /// Estimate the disk space required for a snapshot (memory size + overhead)
    pub async fn snapshot_size_mib(&self) -> anyhow::Result<u32> {
        let info = self.api.vm_info().await?;
        let mem_bytes = info.config.memory.map(|m| m.size).unwrap_or(0);
        // Memory size in MiB plus 1 MiB for state metadata
        Ok((mem_bytes / (1024 * 1024)) as u32 + 1)
    }

    /// Resize the VM (CPUs or memory hotplug)
    pub async fn resize_vm(&self, config: &super::types::VmResizeConfig) -> anyhow::Result<()> {
        self.api.resize_vm(config).await?;
        Ok(())
    }

    /// Notify the hypervisor that a block device's backing store has changed size.
    /// Takes a jail-relative path, constructs the full host path, reads the new size,
    /// and calls the vm.resize-disk API.
    pub async fn update_drive(
        &self,
        drive_id: &str,
        jail_relative_path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<()> {
        // Map drive_id to cloud-hypervisor's auto-generated disk ID format
        let ch_disk_id = match drive_id {
            "root" => "_disk0",
            other => other,
        };

        // Construct full host path from jail root and jail-relative path
        let jail_relative = jail_relative_path.as_ref();
        let host_path = if jail_relative.is_absolute() {
            // Strip leading slash to make it relative to jail root
            let stripped = jail_relative.strip_prefix("/").unwrap_or(jail_relative);
            self.jail_root.join(stripped)
        } else {
            self.jail_root.join(jail_relative)
        };

        // Get the new size of the block device using blockdev --getsize64
        // (metadata.len() returns 0 for block devices)
        let output = tokio::process::Command::new("blockdev")
            .arg("--getsize64")
            .arg(&host_path)
            .output()
            .await
            .with_context(|| format!("Failed to run blockdev --getsize64 on {:?}", host_path))?;

        if !output.status.success() {
            anyhow::bail!(
                "blockdev --getsize64 failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let new_size_bytes: u64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .with_context(|| "Failed to parse blockdev output as u64")?;

        tracing::debug!(
            vm_id = %self.vm_id,
            drive_id = drive_id,
            ch_disk_id = ch_disk_id,
            host_path = %host_path.display(),
            new_size_bytes = new_size_bytes,
            "Notifying cloud-hypervisor of disk resize via vm.resize-disk"
        );

        let config = super::types::VmResizeDiskConfig {
            id: ch_disk_id.to_string(),
            desired_size: new_size_bytes,
        };
        self.api.resize_disk(&config).await?;

        Ok(())
    }
}
