use std::{io, path::PathBuf, process::Stdio, time::Duration};

use crate::{
    cgroup::jailer_cgroup_args,
    data_dir::firecracker::FirecrackerSnapshotPaths,
    process::firecracker::{
        FirecrackerApi, config::get_jail_root_by_vm_id, error::FirecrackerApiError,
        types::FirecrackerInstanceState,
    },
    util::vm_user::{ChownVmError, chown_vm, get_or_create_vm_user},
};

use anyhow::{Context, anyhow};
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use thiserror::Error;
use tokio::process::Command;
use tracing::warn;
use util::linux::UserError;
use uuid::Uuid;
use vers_config::{HypervisorType, VersConfig};

const PID_FILE_NAME: &str = "firecracker.pid";

/// The expected size of a state file. This is actually 16 kiB, but we round up to the nearest MiB for safety.
const STATE_FILE_SIZE_MIB: u32 = 1;

/// Represents a Firecracker process (spawned via jailer)
#[derive(Debug)]
pub struct FirecrackerProcess {
    /// Firecracker API client
    pub api: FirecrackerApi,
    /// PathBuf containing the fully-qualified path to the chroot dir
    pub jail_root: PathBuf,
    /// The VM ID
    pub vm_id: Uuid,
}

#[derive(Error, Debug)]
pub enum FirecrackerProcessError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("user error: {0}")]
    User(#[from] UserError),

    #[error("jailer error: {0}")]
    JailerProcess(io::Error),

    #[error("firecracker error: {0}")]
    FirecrackerApi(#[from] FirecrackerApiError),

    #[error("fs io error: {0}")]
    FsIo(io::Error),

    #[error("timed out waiting for the new firecracker socket")]
    NewFirecrackerSocketTimeout,

    #[error("expected {0} to be valid pid (u32).")]
    PidParsing(String),

    #[error("chown error: {0}")]
    ChownError(#[from] ChownVmError),

    #[error("io error during snapshot creation: {0}")]
    IoDuringSnapshotCreation(io::Error),
}

impl FirecrackerProcess {
    pub async fn new(
        vm_id: Uuid,
        stdout: impl Into<Stdio>,
        stderr: impl Into<Stdio>,
        netns_name: &str,
    ) -> Result<Self, FirecrackerProcessError> {
        let firecracker_bin_path = VersConfig::chelsea().firecracker_bin_path.clone();
        let user_info = get_or_create_vm_user()?;
        let api = FirecrackerApi::new(vm_id)?;

        let cgroup_args = jailer_cgroup_args(&VersConfig::chelsea().vm_cgroup_name);

        // TODO: eliminate setsid https://github.com/hdresearch/chelsea/issues/237
        let _process = Command::new("setsid")
            .arg("jailer")
            .arg("--id")
            .arg(vm_id.to_string())
            .arg("--exec-file")
            .arg(firecracker_bin_path)
            .arg("--uid")
            .arg(user_info.uid.to_string())
            .arg("--gid")
            .arg(user_info.gid.to_string())
            .arg("--netns")
            .arg(format!("/var/run/netns/{}", netns_name))
            .args(&cgroup_args)
            .arg("--")
            .arg("--api-sock")
            .arg(api.socket_path.without_jail_root())
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(false) // False by default; being explicit that we intend to drop this handle, since the process is intended to be independent from the parent.
            .spawn()
            .map_err(FirecrackerProcessError::JailerProcess)?;

        // Wait for the socket file to become available
        let start = std::time::Instant::now();

        loop {
            if api.socket_path.with_jail_root().exists() {
                break;
            } else if start.elapsed()
                >= Duration::from_secs(VersConfig::chelsea().firecracker_socket_timeout_secs)
            {
                return Err(FirecrackerProcessError::NewFirecrackerSocketTimeout);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(Self {
            api,
            jail_root: get_jail_root_by_vm_id(&vm_id),
            vm_id,
        })
    }

    pub async fn pid(&self) -> Result<u32, FirecrackerProcessError> {
        let raw_content = tokio::fs::read_to_string(self.jail_root.join(PID_FILE_NAME))
            .await
            .map_err(FirecrackerProcessError::FsIo)?;
        let content = raw_content.trim();

        match content.parse::<u32>() {
            Ok(pid) => Ok(pid),
            Err(_) => Err(FirecrackerProcessError::PidParsing(content.to_string())),
        }
    }

    pub async fn kill(&self) -> Result<(), FirecrackerProcessError> {
        let pid = self.pid().await?;
        let nix_pid = Pid::from_raw(pid as i32);

        // Kill the process
        let kill_result = signal::kill(nix_pid, Signal::SIGKILL).map_err(|e| {
            FirecrackerProcessError::JailerProcess(std::io::Error::from_raw_os_error(e as i32))
        });
        kill_result?;

        // Wait for the process to actually exit. This is critical because the
        // kernel won't release resources (block devices, network taps, etc.)
        // until the process is fully gone. Without this wait, subsequent RBD
        // unmap calls race against kernel cleanup and fail with EBUSY.
        const WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
        const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
        let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;

        loop {
            match signal::kill(nix_pid, None) {
                Ok(_) => {
                    // Process still exists
                    if tokio::time::Instant::now() >= deadline {
                        tracing::warn!(
                            vm_id = %self.vm_id,
                            pid = pid,
                            "Process still alive after SIGKILL + {WAIT_TIMEOUT:?}; proceeding with cleanup"
                        );
                        break;
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
                Err(nix::errno::Errno::ESRCH) => {
                    // Process is gone
                    tracing::debug!(
                        vm_id = %self.vm_id,
                        pid = pid,
                        "Process exited after SIGKILL"
                    );
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

        // Delete the jail root (The parent is used because the jail root is /srv/jailer/firecracker/{vm_id}/root, and we need to delete the {vm_id} dir)
        let cleanup_result = match self.jail_root.parent() {
            Some(parent) => tokio::fs::remove_dir_all(parent)
                .await
                .map_err(FirecrackerProcessError::JailerProcess),
            None => {
                warn!(
                    vm_id = %self.vm_id,
                    "Could not find parent of jail root for process; removing jail root instead"
                );
                tokio::fs::remove_dir_all(&self.jail_root)
                    .await
                    .map_err(FirecrackerProcessError::JailerProcess)
            }
        };

        cleanup_result?;

        Ok(())
    }

    pub fn process_type(&self) -> HypervisorType {
        HypervisorType::Firecracker
    }

    pub fn vm_id(&self) -> Uuid {
        self.vm_id.clone()
    }

    /// Create a snapshot of the VM and output the files to the snapshots directory; returns, in order, the names of the memory and state files.
    pub async fn create_snapshot(&self, snapshot_id: &Uuid) -> anyhow::Result<(String, String)> {
        let out_dir = &VersConfig::chelsea().snapshot_dir;
        let snapshot_paths = FirecrackerSnapshotPaths::new(&self.vm_id, snapshot_id);

        // Ensure parent directory exists (both share the same parent dir)
        if let Some(parent) = snapshot_paths.mem_file_path.with_jail_root().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating parent directory for mem_file_path")?;
            chown_vm(parent)?;
        }

        // Create the snapshot (inside the jail dir)
        self.api
            .create_snapshot(
                &snapshot_paths.mem_file_path,
                &snapshot_paths.state_file_path,
            )
            .await?;

        // Move files to the output directory
        let mem_file_path_src = snapshot_paths.mem_file_path.with_jail_root();
        let state_file_path_src = snapshot_paths.state_file_path.with_jail_root();

        let mem_file_name = snapshot_paths.mem_file_path.file_name().ok_or(anyhow!(
            "Unable to extract filename from path {}",
            mem_file_path_src.display()
        ))?;
        let state_file_name = snapshot_paths.state_file_path.file_name().ok_or(anyhow!(
            "Unable to extract filename from path {}",
            state_file_path_src.display()
        ))?;

        let mem_file_path_dst = out_dir.join(mem_file_name);
        let state_file_path_dst = out_dir.join(state_file_name);

        tokio::fs::rename(&mem_file_path_src, &mem_file_path_dst).await?;
        tokio::fs::rename(&state_file_path_src, &state_file_path_dst).await?;

        Ok((mem_file_name.to_string(), state_file_name.to_string()))
    }

    pub async fn is_paused(&self) -> anyhow::Result<bool> {
        let status = self.api.describe_instance().await?;
        Ok(status.state == FirecrackerInstanceState::Paused)
    }

    pub async fn pause(&self) -> anyhow::Result<()> {
        self.api.pause_instance().await?;
        Ok(())
    }

    pub async fn resume(&self) -> anyhow::Result<()> {
        self.api.resume_instance().await?;
        Ok(())
    }

    /// Notify Firecracker that the backing block device has changed size by
    /// re-patching the drive with its current path.
    pub async fn update_drive(
        &self,
        drive_id: &str,
        path_on_host: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<()> {
        self.api.patch_drive(drive_id, path_on_host).await?;
        Ok(())
    }

    /// Calculate (or estimate) the disk space, in MiB, required to snapshot this process
    pub async fn snapshot_size_mib(&self) -> anyhow::Result<u32> {
        Ok(self.api.get_machine_configuration().await?.mem_size_mib + STATE_FILE_SIZE_MIB)
    }
}
