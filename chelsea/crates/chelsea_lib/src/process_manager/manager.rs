use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    data_dir::{DataDir, firecracker::FirecrackerSnapshotPaths},
    network::VmNetwork,
    network::linux::namespace::netns_exec,
    process::{
        VmMetadata, VmProcess,
        cloud_hypervisor::{
            CloudHypervisorApi, CloudHypervisorProcess,
            config::{
                CloudHypervisorProcessConfig, CloudHypervisorProcessVsockConfig,
                get_jail_root_by_vm_id as get_ch_jail_root,
            },
            types::{
                CpusConfig, DiskConfig, KernelConfig, MemoryConfig, NetConfig, VmRestoreConfig,
                VsockConfig,
            },
        },
        firecracker::{
            FirecrackerApi, FirecrackerProcess,
            config::{
                FirecrackerProcessConfig, FirecrackerProcessVsockConfig, get_jail_root_by_vm_id,
            },
        },
    },
    process_manager::{
        VmCloudHypervisorProcessCommitFilepaths, VmCloudHypervisorProcessCommitMetadata,
        VmFirecrackerProcessCommitFilepaths, VmProcessCommitMetadata, VmProcessConfig,
        VmProcessManagerStore,
        commit::VmFirecrackerProcessCommitMetadata,
        sleep_snapshot::{
            VmCloudHypervisorProcessSleepSnapshotFilepaths,
            VmFirecrackerProcessSleepSnapshotFilepaths,
        },
        store::VmProcessRecord,
    },
    util::vm_user::chown_vm,
    vm::VmConfig,
    vm_manager::commit::VmCommitMetadata,
    vsock::VsockClient,
};
use anyhow::{Context, anyhow, bail};
use ssh_key::{PrivateKey, PublicKey};
use tracing::{error, warn};
use util::{defer::DeferAsync, exec_ssh, join_errors, linux::copy_device_node};
use uuid::Uuid;
use vers_config::{HypervisorType, VersConfig};
use vers_pg::{
    db::VersPg,
    schema::chelsea::tables::sleep_snapshot::{
        RecordCloudHypervisorProcessSleepSnapshot, RecordFirecrackerProcessSleepSnapshot,
        RecordProcessSleepSnapshot,
    },
};

/// Get the vsock socket path for a VM based on hypervisor type.
fn vsock_socket_path(vm_id: &Uuid, hypervisor_type: HypervisorType) -> PathBuf {
    match hypervisor_type {
        HypervisorType::Firecracker => FirecrackerProcessVsockConfig::with_defaults(*vm_id)
            .uds_path
            .with_jail_root(),
        HypervisorType::CloudHypervisor => CloudHypervisorProcessVsockConfig::with_defaults(*vm_id)
            .socket_path
            .with_jail_root(),
    }
}

/// Write VM metadata files (/etc/vminfo and /etc/vm_id) via vsock.
/// Retries with exponential backoff since the vsock connection may not be ready
/// immediately after VM boot or snapshot restore.
async fn write_vm_metadata_via_vsock(
    vm_id: &Uuid,
    hypervisor_type: HypervisorType,
) -> anyhow::Result<()> {
    let socket_path = vsock_socket_path(vm_id, hypervisor_type);
    let vsock_client = VsockClient::new(&socket_path);

    let metadata = VmMetadata { vm_id: *vm_id };
    let metadata_str = serde_json::to_string_pretty(&metadata).context("Serializing VmMetadata")?;

    // Retry with exponential backoff - vsock may not be ready immediately after boot/restore
    let mut last_error = None;
    for attempt in 1..=10 {
        match vsock_client
            .write_file("/etc/vminfo", metadata_str.as_bytes(), 0o644, true)
            .await
        {
            Ok(_) => {
                last_error = None;
                break;
            }
            Err(e) => {
                tracing::warn!(
                    vm_id = %vm_id,
                    attempt = attempt,
                    socket_path = %socket_path.display(),
                    error = %e,
                    "Vsock write_file failed, will retry"
                );
                last_error = Some(e);
                if attempt < 10 {
                    let delay_ms = std::cmp::min(100 * (1 << attempt), 5000);
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    if let Some(err) = last_error {
        return Err(err).context(format!(
            "Writing /etc/vminfo via vsock after 10 retries (socket: {})",
            socket_path.display()
        ));
    }

    vsock_client
        .write_file("/etc/vm_id", vm_id.to_string().as_bytes(), 0o644, true)
        .await
        .context("Writing /etc/vm_id via vsock")?;

    Ok(())
}

/// A higher-level interface to spawning VM processes with a persistence layer
#[derive(Clone)]
pub struct VmProcessManager {
    /// The manager's local store
    pub local_store: Arc<dyn VmProcessManagerStore>,
    /// The manager's remote store
    pub remote_store: Arc<VersPg>,
}

impl VmProcessManager {
    pub fn new(local_store: Arc<dyn VmProcessManagerStore>, remote_store: Arc<VersPg>) -> Self {
        Self {
            local_store,
            remote_store,
        }
    }

    /// Rehydrate a VmProcess using the manager's stores
    pub async fn rehydrate_vm_process(&self, pid: u32) -> anyhow::Result<Arc<VmProcess>> {
        let process_record = self
            .local_store
            .fetch_vm_process_record(pid)
            .await?
            .ok_or(anyhow!("Failed to find VmProcess with PID {}", pid))?;

        let process = match process_record.process_type {
            HypervisorType::Firecracker => VmProcess::Firecracker(FirecrackerProcess {
                jail_root: get_jail_root_by_vm_id(&process_record.vm_id),
                api: FirecrackerApi::new(process_record.vm_id.clone())?,
                vm_id: process_record.vm_id,
            }),
            HypervisorType::CloudHypervisor => VmProcess::CloudHypervisor(CloudHypervisorProcess {
                jail_root: get_ch_jail_root(&process_record.vm_id),
                api: CloudHypervisorApi::new(process_record.vm_id.clone())?,
                vm_id: process_record.vm_id,
            }),
        };

        Ok(Arc::new(process))
    }

    /// Rehydrate a VmProcessCommitMetadata using the manager's stores
    pub async fn rehydrate_process_commit_metadata(&self) -> VmProcessCommitMetadata {
        // Process commit metadata currently doesn't exist; stubbed out for consistency
        VmProcessCommitMetadata::Firecracker(VmFirecrackerProcessCommitMetadata {})
    }

    /// Spawn a new VM process of the given type
    pub async fn spawn_new(
        &self,
        vm_id: Uuid,
        vm_config: &VmConfig,
        process_config: &VmProcessConfig,
        netns_name: &str,
    ) -> anyhow::Result<Arc<VmProcess>> {
        // Spawn the process based on the type of process_config
        let vm_process = match process_config {
            VmProcessConfig::Firecracker(process_config) => {
                self.spawn_new_firecracker(vm_id, vm_config, process_config, netns_name)
                    .await?
            }
            VmProcessConfig::CloudHypervisor(process_config) => {
                self.spawn_new_cloud_hypervisor(vm_id, vm_config, process_config, netns_name)
                    .await?
            }
        };

        // Defer killing the process
        let mut defer = DeferAsync::new();
        defer.defer({
            let vm_process = vm_process.clone();
            async move {
                if let Err(error) = vm_process.kill().await {
                    error!(%error, "Error while cleaning up VM process");
                }
            }
        });

        // Store the process in the manager's store
        let record = VmProcessRecord::try_from_vm_process(&vm_process).await?;
        self.local_store.insert_vm_process_record(&record).await?;

        defer.commit();
        Ok(vm_process)
    }

    /// Spawns a VmProcess given information stored previously in a VmProcessCommitMetadata struct. Expects the commit files to have been downloaded by the caller.
    /// Expects the VM's committed SSH private key to be passed in separately; the keypair in the VmConfig should be the new keypair.
    pub async fn spawn_from_commit(
        &self,
        vm_id: Uuid,
        vm_config: &VmConfig,
        vm_commit_metadata: &VmCommitMetadata,
        vm_network: &VmNetwork,
        vm_drive_path: &Path,
        committed_ssh_private_key: &PrivateKey,
    ) -> anyhow::Result<Arc<VmProcess>> {
        // Spawn a VmProcess based on the type of the commit metadata
        let vm_process = match &vm_commit_metadata.process_metadata {
            VmProcessCommitMetadata::Firecracker(_) => {
                self.spawn_from_commit_firecracker(
                    vm_id,
                    vm_config,
                    vm_commit_metadata,
                    vm_network,
                    vm_drive_path,
                    committed_ssh_private_key,
                )
                .await?
            }
            VmProcessCommitMetadata::CloudHypervisor(_) => {
                self.spawn_from_commit_cloud_hypervisor(
                    vm_id,
                    vm_config,
                    vm_commit_metadata,
                    vm_network,
                    vm_drive_path,
                    committed_ssh_private_key,
                )
                .await?
            }
        };

        // Defer killing the process
        let mut defer = DeferAsync::new();
        defer.defer({
            let vm_process = vm_process.clone();
            async move {
                if let Err(error) = vm_process.kill().await {
                    error!(%error, "Error while cleaning up VM process");
                }
            }
        });

        // Store the process in the manager's store
        let record = VmProcessRecord::try_from_vm_process(&vm_process).await?;
        self.local_store.insert_vm_process_record(&record).await?;

        defer.commit();
        Ok(vm_process)
    }

    /// Spawns a VmProcess given information stored previously in a RecordProcessSleepSnapshot struct. Expects the snapshot files to have been downloaded by the caller.
    pub async fn spawn_from_sleep_snapshot(
        &self,
        vm_id: &Uuid,
        snapshot_id: &Uuid,
        vm_config: &VmConfig,
        vm_process_sleep_snapshot_metadata: &RecordProcessSleepSnapshot,
        vm_network: &VmNetwork,
        vm_drive_path: &Path,
    ) -> anyhow::Result<Arc<VmProcess>> {
        // Spawn a VmProcess based on the type of the commit metadata
        let vm_process = match &vm_process_sleep_snapshot_metadata {
            RecordProcessSleepSnapshot::Firecracker(vm_process_sleep_snapshot_metadata) => {
                self.spawn_from_sleep_snapshot_firecracker(
                    vm_id,
                    snapshot_id,
                    vm_config,
                    vm_process_sleep_snapshot_metadata,
                    vm_network,
                    vm_drive_path,
                )
                .await?
            }
            RecordProcessSleepSnapshot::CloudHypervisor(vm_process_sleep_snapshot_metadata) => {
                self.spawn_from_sleep_snapshot_cloud_hypervisor(
                    *vm_id,
                    vm_config,
                    vm_process_sleep_snapshot_metadata,
                    vm_network,
                    vm_drive_path,
                )
                .await?
            }
        };

        // Defer killing the process
        let mut defer = DeferAsync::new();
        defer.defer({
            let vm_process = vm_process.clone();
            async move {
                if let Err(error) = vm_process.kill().await {
                    error!(%error, "Error while cleaning up VM process");
                }
            }
        });

        // Store the process in the manager's store
        let record = VmProcessRecord::try_from_vm_process(&vm_process).await?;
        self.local_store.insert_vm_process_record(&record).await?;

        defer.commit();
        Ok(vm_process)
    }

    async fn spawn_from_sleep_snapshot_firecracker(
        &self,
        vm_id: &Uuid,
        snapshot_id: &Uuid,
        _vm_config: &VmConfig,
        _vm_process_sleep_snapshot_metadata: &RecordFirecrackerProcessSleepSnapshot,
        vm_network: &VmNetwork,
        vm_drive_path: &Path,
    ) -> anyhow::Result<Arc<VmProcess>> {
        let vm_root_drive_path = VersConfig::chelsea().vm_root_drive_path.clone();
        // Defer cleaning up the jail root
        let jail_path = get_jail_root_by_vm_id(&vm_id);
        let mut defer = DeferAsync::new();
        defer.defer({
            async move {
                if let Err(error) = tokio::fs::remove_dir_all(jail_path).await {
                    error!(%error, "Error cleaning up jail root for new Firecracker process");
                }
            }
        });

        // Copy the drive device node to the jail
        let dest_path = get_jail_root_by_vm_id(&vm_id).join(&vm_root_drive_path);
        copy_device_node(vm_drive_path, &dest_path)
            .await
            .context("Copying VM drive device node")?;
        chown_vm(dest_path).context("chown device node")?;

        // Create the snapshot dir for linking the Firecracker snapshot to
        let dest_snapshot_paths = FirecrackerSnapshotPaths::new(vm_id, snapshot_id);
        if let Some(parent) = dest_snapshot_paths.mem_file_path.with_jail_root().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating VM snapshot directory at jail root")?;
        }

        // Derive the expected location for the source snapshot files from the snapshot ID
        let src_snapshot_paths =
            VmFirecrackerProcessSleepSnapshotFilepaths::from_snapshot_id(&snapshot_id.to_string())
                .await?;

        // Hard link snapshot to the new jail dir
        tokio::fs::hard_link(
            src_snapshot_paths.mem_file_path,
            dest_snapshot_paths.mem_file_path.with_jail_root(),
        )
        .await
        .context("Hard linking commit memory snapshot to VM jail dir")?;
        chown_vm(dest_snapshot_paths.mem_file_path.with_jail_root())
            .context("Chown memory snapshot")?;

        tokio::fs::hard_link(
            src_snapshot_paths.state_file_path,
            dest_snapshot_paths.state_file_path.with_jail_root(),
        )
        .await
        .context("Hard linking commit state snapshot to VM jail dir")?;
        chown_vm(dest_snapshot_paths.state_file_path.with_jail_root())
            .context("Chown state snapshot")?;

        // Create the vsock socket directory so Firecracker can bind on resume
        let vsock_config = FirecrackerProcessVsockConfig::with_defaults(vm_id.clone());
        if let Some(parent) = vsock_config.uds_path.with_jail_root().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating parent directory for vsock uds_path")?;
        }

        // Create stdout and stderr logs in the data directory
        let data_dir = DataDir::global();
        let (stdout_file, stderr_file) = data_dir
            .create_process_logs(&vm_id)
            .await
            .context("Creating process logs")?;

        // Spawn the Firecracker process
        let firecracker_process = FirecrackerProcess::new(
            vm_id.clone(),
            stdout_file.into_std().await,
            stderr_file.into_std().await,
            &vm_network.netns_name,
        )
        .await
        .context("Spawning Firecracker process")?;

        // Load snapshot and resume
        firecracker_process
            .api
            .load_snapshot(
                &dest_snapshot_paths.state_file_path,
                &dest_snapshot_paths.mem_file_path,
                false,
            )
            .await?;

        firecracker_process.api.resume_instance().await?;

        // Wrap in Arc<VmProcess> for return
        let process = Arc::new(VmProcess::Firecracker(firecracker_process));

        // Defer cleaning up the process
        defer.defer({
            let process = process.clone();
            async move {
                if let Err(error) = process.kill().await {
                    error!(%error, "Error cleaning up Firecracker process");
                }
            }
        });

        // Write VmMetadata to /etc/vminfo and ID to /etc/vm_id via vsock
        write_vm_metadata_via_vsock(&process.vm_id(), HypervisorType::Firecracker).await?;

        defer.commit();
        Ok(process)
    }

    /// Prepares for and spawns a new Firecracker VM process
    async fn spawn_new_firecracker(
        &self,
        vm_id: Uuid,
        _vm_config: &VmConfig,
        process_config: &FirecrackerProcessConfig,
        netns_name: &str,
    ) -> anyhow::Result<Arc<VmProcess>> {
        let vm_root_drive_path = VersConfig::chelsea().vm_root_drive_path.clone();
        // Defer cleaning up the jail root
        let jail_path = get_jail_root_by_vm_id(&vm_id);
        let mut defer = DeferAsync::new();
        defer.defer({
            async move {
                if let Err(error) = tokio::fs::remove_dir_all(jail_path).await {
                    error!(%error, "Error cleaning up jail root for new Firecracker process");
                }
            }
        });

        // Copy the drive device node to the process
        let drive_path = &process_config.drive.path_on_host;
        let dest_path = get_jail_root_by_vm_id(&vm_id).join(&vm_root_drive_path);
        copy_device_node(drive_path, &dest_path)
            .await
            .context("Copying drive device node")?;
        chown_vm(dest_path)?;

        // Check if kernel exists
        let kernel_path = &process_config.boot_source.kernel_path;
        let kernel_src = kernel_path.without_jail_root();
        let kernel_dst = kernel_path.with_jail_root();

        if !kernel_src.exists() {
            bail!("No kernel found at path {}", kernel_src.display());
        }

        // Copy the kernel to the jail root
        if let Some(parent) = kernel_dst.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Kernel parent dir create")?;
        } else {
            warn!(?kernel_dst, "No kernel destination parent directory?");
        }
        tokio::fs::copy(&kernel_src, &kernel_dst)
            .await
            .context(format!("Copying kernel {}", kernel_src.display()))?;
        chown_vm(kernel_dst).context("Kernel chown")?;

        // NOTE: SSH keys and VM ID are now passed via kernel command line (chelsea_ssh_pubkey, chelsea_vm_id)
        // and configured by ssh-setup.sh and notify-ready.sh scripts in the VM at boot time.
        // This eliminates the need to mount/write/unmount the volume here, saving ~250ms.

        // Create the vsock socket directory in the jail root
        let vsock_config = FirecrackerProcessVsockConfig::with_defaults(vm_id);
        if let Some(parent) = vsock_config.uds_path.with_jail_root().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating vsock socket directory")?;
            chown_vm(parent).context("Chown vsock socket directory")?;
        }

        // Create stdout and stderr logs in the data directory
        let data_dir = DataDir::global();
        let (stdout_file, stderr_file) = data_dir
            .create_process_logs(&vm_id)
            .await
            .context("Creating process logs")?;

        // Spawn the Firecracker process
        let firecracker_process = FirecrackerProcess::new(
            vm_id.clone(),
            stdout_file.into_std().await,
            stderr_file.into_std().await,
            netns_name,
        )
        .await
        .context("Spawning Firecracker process")?;

        // Configure the newly-spawned process via API
        let api = &firecracker_process.api;
        let vm_root_drive_path_clone = vm_root_drive_path.clone();

        let (result_boot, result_drive, result_machine, result_net, result_vsock) = tokio::join!(
            async {
                let config = &process_config.boot_source;
                api.configure_boot_source(&config.kernel_path, &config.boot_args)
                    .await
            },
            async {
                let config = &process_config.drive;
                api.configure_drive(
                    &config.drive_id,
                    &vm_root_drive_path_clone,
                    config.is_root_device,
                    config.is_read_only,
                )
                .await
            },
            async { api.configure_machine(&process_config.machine).await },
            async {
                let config = &process_config.network;
                api.configure_network(&config.iface_id, &config.host_dev_name, &config.guest_mac)
                    .await
            },
            async {
                api.configure_vsock(
                    &vsock_config.vsock_id,
                    vsock_config.guest_cid,
                    &vsock_config.uds_path,
                )
                .await
            },
        );

        // Collect errors
        let errors = vec![
            result_boot,
            result_drive,
            result_machine,
            result_net,
            result_vsock,
        ]
        .into_iter()
        .filter_map(|result| result.err())
        .collect::<Vec<_>>();

        if errors.len() > 0 {
            anyhow::bail!(
                "One or more Firecracker API configuration errors: {}",
                join_errors(&errors, "; ")
            );
        }

        // Start the process
        api.start_instance().await?;

        // Wrap in Arc<VmProcess> for return
        let process = Arc::new(VmProcess::Firecracker(firecracker_process));

        // Defer cleaning up the process
        defer.defer({
            let process = process.clone();
            async move {
                if let Err(error) = process.kill().await {
                    error!(%error, "Error cleaning up Firecracker process");
                }
            }
        });

        defer.commit();
        Ok(process)
    }

    async fn spawn_from_commit_firecracker(
        &self,
        vm_id: Uuid,
        vm_config: &VmConfig,
        vm_commit_metadata: &VmCommitMetadata,
        vm_network: &VmNetwork,
        vm_drive_path: &Path,
        committed_ssh_private_key: &PrivateKey,
    ) -> anyhow::Result<Arc<VmProcess>> {
        let vm_root_drive_path = VersConfig::chelsea().vm_root_drive_path.clone();
        // Defer cleaning up the jail root
        let jail_path = get_jail_root_by_vm_id(&vm_id);
        let mut defer = DeferAsync::new();
        defer.defer({
            async move {
                if let Err(error) = tokio::fs::remove_dir_all(jail_path).await {
                    error!(%error, "Error cleaning up jail root for new Firecracker process");
                }
            }
        });

        // Copy the drive device node to the process
        {
            let dest_path = get_jail_root_by_vm_id(&vm_id).join(&vm_root_drive_path);
            copy_device_node(vm_drive_path, &dest_path)
                .await
                .context("Copying VM drive device node")?;
            chown_vm(dest_path).context("chown device node")?;
        }

        // Create the snapshot dir for linking the commit snapshot to
        let vm_snapshot_paths =
            FirecrackerSnapshotPaths::new(&vm_id, &vm_commit_metadata.commit_id);
        if let Some(parent) = vm_snapshot_paths.mem_file_path.with_jail_root().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating VM snapshot directory at jail root")?;
        }

        // Derive the expected location for the commit files from the commit ID
        let commit_snapshot_paths =
            VmFirecrackerProcessCommitFilepaths::from_commit_id(&vm_commit_metadata.commit_id)
                .await?;

        // Hard link snapshot to the new jail dir
        tokio::fs::hard_link(
            commit_snapshot_paths.mem_file_path,
            vm_snapshot_paths.mem_file_path.with_jail_root(),
        )
        .await
        .context("Hard linking commit memory snapshot to VM jail dir")?;
        chown_vm(vm_snapshot_paths.mem_file_path.with_jail_root())
            .context("Chown memory snapshot")?;

        tokio::fs::hard_link(
            commit_snapshot_paths.state_file_path,
            vm_snapshot_paths.state_file_path.with_jail_root(),
        )
        .await
        .context("Hard linking commit state snapshot to VM jail dir")?;
        chown_vm(vm_snapshot_paths.state_file_path.with_jail_root())
            .context("Chown state snapshot")?;

        // Create the vsock socket directory so Firecracker can bind on resume
        let vsock_config = FirecrackerProcessVsockConfig::with_defaults(vm_id);
        if let Some(parent) = vsock_config.uds_path.with_jail_root().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating vsock socket directory for commit restore")?;
            chown_vm(parent).context("Chown vsock socket directory")?;
        }

        // Create stdout and stderr logs in the data directory
        let data_dir = DataDir::global();
        let (stdout_file, stderr_file) = data_dir
            .create_process_logs(&vm_id)
            .await
            .context("Creating process logs")?;

        // Spawn the Firecracker process
        let firecracker_process = FirecrackerProcess::new(
            vm_id.clone(),
            stdout_file.into_std().await,
            stderr_file.into_std().await,
            &vm_network.netns_name,
        )
        .await
        .context("Spawning Firecracker process")?;

        // Load snapshot and resume
        firecracker_process
            .api
            .load_snapshot(
                &vm_snapshot_paths.state_file_path,
                &vm_snapshot_paths.mem_file_path,
                false,
            )
            .await?;

        firecracker_process.api.resume_instance().await?;

        // Wrap in Arc<VmProcess> for return
        let process = Arc::new(VmProcess::Firecracker(firecracker_process));

        // Defer cleaning up the process
        defer.defer({
            let process = process.clone();
            async move {
                if let Err(error) = process.kill().await {
                    error!(%error, "Error cleaning up Firecracker process");
                }
            }
        });

        // Use the originally-committed key to install the newly generated SSH key before switching credentials.
        let new_public_key = PublicKey::from(vm_config.ssh_keypair.public.clone())
            .to_openssh()?
            .replace('\'', "'\\''");
        let install_key_cmd = format!(
            "mkdir -p /root/.ssh && chmod 700 /root/.ssh && printf '%s\\n' '{pub}' > /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys",
            pub = new_public_key,
        );

        let vm_host = vm_network.vm_addr.to_string();
        exec_ssh(committed_ssh_private_key, &vm_host, &install_key_cmd)
            .await
            .context("Installing new SSH public key via committed credentials")?;

        // Write VmMetadata to /etc/vminfo and ID to /etc/vm_id via vsock
        write_vm_metadata_via_vsock(&process.vm_id(), HypervisorType::Firecracker).await?;

        defer.commit();
        Ok(process)
    }

    /// Commits the process, returning a vec of filenames created in `commit_dir`
    pub async fn commit_process(
        &self,
        pid: u32,
        commit_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, VmProcessCommitMetadata)> {
        let process = self.rehydrate_vm_process(pid).await?;

        let (process_to_upload, process_commit_metadata) = match process.as_ref() {
            VmProcess::Firecracker(process) => {
                self.commit_firecracker_process(&process, commit_id).await?
            }
            VmProcess::CloudHypervisor(process) => {
                self.commit_cloud_hypervisor_process(process, commit_id)
                    .await?
            }
        };

        Ok((process_to_upload, process_commit_metadata))
    }

    /// Commits a Firecracker process, creating state and memory files in `commit_dir` and returning their filenames
    pub async fn commit_firecracker_process(
        &self,
        process: &FirecrackerProcess,
        commit_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, VmProcessCommitMetadata)> {
        // Create snapshot in the data directory
        let (mem_file_name, state_file_name) = process.create_snapshot(commit_id).await?;

        Ok((
            vec![mem_file_name, state_file_name],
            VmProcessCommitMetadata::Firecracker(VmFirecrackerProcessCommitMetadata {}),
        ))
    }

    /// Commits a CloudHypervisor process, creating a snapshot tar archive and returning its filename
    pub async fn commit_cloud_hypervisor_process(
        &self,
        process: &CloudHypervisorProcess,
        commit_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, VmProcessCommitMetadata)> {
        // Create snapshot (returns tar archive filename)
        let snapshot_file_name = process.create_snapshot(&commit_id.to_string()).await?;

        Ok((
            vec![snapshot_file_name],
            VmProcessCommitMetadata::CloudHypervisor(VmCloudHypervisorProcessCommitMetadata {}),
        ))
    }

    /// Makes a sleep snapshot of the process, returning a vec of filenames created in `snapshot_dir`
    pub async fn sleep_snapshot_process(
        &self,
        pid: u32,
        snapshot_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, RecordProcessSleepSnapshot)> {
        let process = self.rehydrate_vm_process(pid).await?;

        let (process_to_upload, process_sleep_snapshot_metadata) = match process.as_ref() {
            VmProcess::Firecracker(process) => {
                self.sleep_snapshot_firecracker_process(&process, snapshot_id)
                    .await?
            }
            VmProcess::CloudHypervisor(process) => {
                self.sleep_snapshot_cloud_hypervisor_process(process)
                    .await?
            }
        };

        Ok((process_to_upload, process_sleep_snapshot_metadata))
    }

    /// Makes a sleep snapshot of a Firecracker process, creating state and memory files in `snapshot_dir` and returning their filenames
    pub async fn sleep_snapshot_firecracker_process(
        &self,
        process: &FirecrackerProcess,
        snapshot_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, RecordProcessSleepSnapshot)> {
        // Create snapshot in the data directory
        let (mem_file_name, state_file_name) = process.create_snapshot(snapshot_id).await?;

        Ok((
            vec![mem_file_name, state_file_name],
            RecordProcessSleepSnapshot::Firecracker(RecordFirecrackerProcessSleepSnapshot {}),
        ))
    }

    /// Makes a sleep snapshot of a CloudHypervisor process, creating a snapshot tar archive and returning its filename
    pub async fn sleep_snapshot_cloud_hypervisor_process(
        &self,
        process: &CloudHypervisorProcess,
    ) -> anyhow::Result<(Vec<String>, RecordProcessSleepSnapshot)> {
        // Create snapshot (returns tar archive filename)
        let snapshot_file_name = process.create_snapshot(&process.vm_id.to_string()).await?;

        Ok((
            vec![snapshot_file_name],
            RecordProcessSleepSnapshot::CloudHypervisor(
                RecordCloudHypervisorProcessSleepSnapshot {},
            ),
        ))
    }

    /// Calculate (or estimate) the disk space, in MiB, required to write commit files
    pub async fn calculate_commit_size_mib(&self, process_id: u32) -> anyhow::Result<u32> {
        let process = self.rehydrate_vm_process(process_id).await?;
        match process.as_ref() {
            VmProcess::Firecracker(process) => process.snapshot_size_mib().await,
            VmProcess::CloudHypervisor(process) => process.snapshot_size_mib().await,
        }
    }

    /// Calculate (or estimate) the disk space, in MiB, required to write snapshot files
    pub async fn calculate_sleep_snapshot_size_mib(&self, process_id: u32) -> anyhow::Result<u32> {
        let process = self.rehydrate_vm_process(process_id).await?;
        match process.as_ref() {
            VmProcess::Firecracker(process) => process.snapshot_size_mib().await,
            VmProcess::CloudHypervisor(process) => process.snapshot_size_mib().await,
        }
    }

    /// Kill the specified VM process and remove its record from the VmProcessorManager's store
    pub async fn kill(&self, vm_process_pid: u32) -> anyhow::Result<()> {
        let process = self.rehydrate_vm_process(vm_process_pid).await?;

        let task_kill_process = process.kill();
        let task_delete_record = self.local_store.delete_vm_process_record(vm_process_pid);

        let results = [
            task_kill_process.await.map_err(anyhow::Error::from),
            task_delete_record.await.map_err(anyhow::Error::from),
        ];

        let errors = results
            .into_iter()
            .filter_map(|result| result.err())
            .collect::<Vec<_>>();

        match errors.is_empty() {
            true => Ok(()),
            false => Err(anyhow!(join_errors(&errors, ", "))),
        }
    }

    /// Callback invoked when a process's parent VM is resumed
    pub async fn on_vm_resumed(&self, vm_process_pid: u32) -> anyhow::Result<()> {
        let process = self.rehydrate_vm_process(vm_process_pid).await?;
        process.resume().await
    }

    /// Callback invoked when a process's parent VM is put to sleep
    pub async fn on_vm_sleep(&self, vm_process_pid: u32) -> anyhow::Result<()> {
        let process = self.rehydrate_vm_process(vm_process_pid).await?;

        let task_kill_process = process.kill();
        let task_delete_record = self.local_store.delete_vm_process_record(vm_process_pid);

        let results = [
            task_kill_process.await.map_err(anyhow::Error::from),
            task_delete_record.await.map_err(anyhow::Error::from),
        ];

        let errors = results
            .into_iter()
            .filter_map(|result| result.err())
            .collect::<Vec<_>>();

        match errors.is_empty() {
            true => Ok(()),
            false => Err(anyhow!(join_errors(&errors, ", "))),
        }
    }

    /// Spawns a new CloudHypervisor VM process
    async fn spawn_new_cloud_hypervisor(
        &self,
        vm_id: Uuid,
        vm_config: &VmConfig,
        process_config: &CloudHypervisorProcessConfig,
        netns_name: &str,
    ) -> anyhow::Result<Arc<VmProcess>> {
        let vm_root_drive_path = VersConfig::chelsea().vm_root_drive_path.clone();

        // Create jail directory structure first
        let jail_path = get_ch_jail_root(&vm_id);
        tokio::fs::create_dir_all(&jail_path)
            .await
            .context("Creating jail root directory")?;

        // Defer cleaning up the jail root
        let mut defer = DeferAsync::new();
        defer.defer({
            let jail_path = jail_path.clone();
            async move {
                if let Err(error) = tokio::fs::remove_dir_all(jail_path).await {
                    error!(%error, "Error cleaning up jail root for new Cloud Hypervisor process");
                }
            }
        });

        // Copy the drive device node to the jail
        let drive_path = &process_config.disk.path;
        let dest_path = get_ch_jail_root(&vm_id).join(&vm_root_drive_path);
        copy_device_node(drive_path, &dest_path)
            .await
            .context("Copying drive device node")?;
        chown_vm(dest_path)?;

        // Copy the kernel to the jail root
        let kernel_path = &process_config.kernel.path;
        let kernel_dest = kernel_path.with_jail_root();
        if let Some(parent) = kernel_dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Kernel parent dir create")?;
        }
        // Get the actual kernel source path from data_dir
        let data_dir = DataDir::global();
        let kernel_src = data_dir.kernel_dir.join(&vm_config.kernel_name);

        // Check if source kernel exists and get its size
        let kernel_metadata = tokio::fs::metadata(&kernel_src).await.with_context(|| {
            format!(
                "Kernel source file not found or inaccessible: {:?}",
                kernel_src
            )
        })?;

        if kernel_metadata.len() == 0 {
            anyhow::bail!(
                "Kernel source file is empty: {:?} (size: 0 bytes). \
                Please ensure a valid kernel file is present.",
                kernel_src
            );
        }

        tracing::debug!(
            "Copying kernel from {:?} to {:?} (size: {} bytes)",
            kernel_src,
            kernel_dest,
            kernel_metadata.len()
        );

        tokio::fs::copy(&kernel_src, &kernel_dest)
            .await
            .with_context(|| {
                format!(
                    "Failed to copy kernel from {:?} to {:?}",
                    kernel_src, kernel_dest
                )
            })?;
        chown_vm(&kernel_dest).context("Kernel chown")?;

        // NOTE: SSH keys and VM ID are passed via kernel command line (chelsea_ssh_pubkey, chelsea_vm_id)
        // and configured by ssh-setup.sh and notify-ready.sh scripts in the VM at boot time.
        // This eliminates the need to mount/write/unmount the volume here, saving ~250ms.

        // Create stdout and stderr logs in the data directory
        let data_dir = DataDir::global();
        let (stdout_file, stderr_file) = data_dir
            .create_process_logs(&vm_id)
            .await
            .context("Creating process logs")?;

        // Create empty file for logger
        if let Some(parent) = process_config.log_file.with_jail_root().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating parent directory for logger log_path")?;
        }
        let logger_log_path = process_config.log_file.with_jail_root();
        tokio::fs::write(logger_log_path.clone(), &[])
            .await
            .context("Touching empty logger log file")?;
        chown_vm(logger_log_path.clone()).context("Chown logger log file")?;

        // Spawn the CloudHypervisor process
        tracing::info!(vm_id = %vm_id, "Spawning cloud-hypervisor process");
        let ch_process = CloudHypervisorProcess::new(
            vm_id.clone(),
            stdout_file.into_std().await,
            stderr_file.into_std().await,
            netns_name,
        )
        .await?;

        // Verify process is running
        let pid = ch_process
            .pid()
            .await
            .context("Getting cloud-hypervisor PID")?;
        tracing::info!(vm_id = %vm_id, pid = pid, "Cloud-hypervisor process spawned successfully");

        // Configure the newly-spawned process via API
        let api = &ch_process.api;

        // Prepare VM configuration for API
        let cpus = CpusConfig {
            boot_vcpus: process_config.cpus.boot_vcpus,
            max_vcpus: Some(process_config.cpus.boot_vcpus),
        };

        let memory = MemoryConfig {
            size: (process_config.memory.size_mib as u64) * 1024 * 1024,
        };

        let kernel = KernelConfig {
            path: kernel_path.with_jail_root().to_string_lossy().to_string(),
            cmdline: Some(process_config.kernel.cmdline.clone()),
        };

        // Use the jailed disk path where we copied the device node
        let disk_path = get_ch_jail_root(&vm_id).join(&vm_root_drive_path);
        tracing::info!(
            vm_id = %vm_id,
            disk_path = %disk_path.display(),
            "Cloud Hypervisor disk path for VM"
        );
        let disks = vec![DiskConfig {
            path: disk_path.to_string_lossy().to_string(),
            readonly: Some(process_config.disk.readonly),
            direct: None,
            id: None,
        }];

        // Cloud Hypervisor works with a simple tap device (no special flags needed).
        // The network manager already created the tap, so we don't need to recreate it.

        // Don't specify num_queues - let CH use defaults
        let net = vec![NetConfig {
            tap: Some(process_config.network.tap.clone()),
            mac: Some(process_config.network.mac.to_string()),
            num_queues: None,
        }];

        // Configure vsock for host-guest communication
        let vsock_config = CloudHypervisorProcessVsockConfig::with_defaults(vm_id.clone());
        let vsock = VsockConfig {
            cid: vsock_config.guest_cid,
            socket: vsock_config
                .socket_path
                .with_jail_root()
                .to_string_lossy()
                .to_string(),
            id: Some("vsock0".to_string()),
        };

        // Create and boot the VM via API
        tracing::info!(vm_id = %vm_id, "Calling cloud-hypervisor API: vm.create");
        if let Err(e) = api
            .create_vm(&cpus, &memory, &kernel, &disks, &net, None, Some(&vsock))
            .await
        {
            tracing::error!(
                vm_id = %vm_id,
                error = %e,
                "Cloud Hypervisor API: vm.create failed with error: {}",
                e
            );
            return Err(anyhow::anyhow!(
                "Cloud Hypervisor API: vm.create failed: {}",
                e
            ));
        }
        tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM created successfully");

        tracing::info!(vm_id = %vm_id, "Calling cloud-hypervisor API: vm.boot");
        if let Err(e) = api.boot_vm().await {
            tracing::error!(
                vm_id = %vm_id,
                error = %e,
                "Cloud Hypervisor API: vm.boot failed with error: {}",
                e
            );
            return Err(anyhow::anyhow!(
                "Cloud Hypervisor API: vm.boot failed: {}",
                e
            ));
        }
        tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM booted successfully");

        // Wrap in Arc<VmProcess> for return
        let process = Arc::new(VmProcess::CloudHypervisor(ch_process));

        // Defer cleaning up the process
        defer.defer({
            let process = process.clone();
            async move {
                if let Err(error) = process.kill().await {
                    error!(%error, "Error cleaning up Cloud Hypervisor process");
                }
            }
        });

        defer.commit();
        Ok(process)
    }

    /// Spawns a CloudHypervisor VM from a commit
    /// Patch the Cloud Hypervisor snapshot's config.json to update paths that change between VMs.
    /// This is needed because CH stores absolute paths (disk, kernel, vsock socket) in the snapshot config,
    /// but on restore the VM has a new ID, new disk device, and new jail root.
    async fn patch_ch_snapshot_config(
        restore_dir: &Path,
        new_disk_path: &Path,
        new_kernel_path: &Path,
        new_vsock_socket_path: &Path,
    ) -> anyhow::Result<()> {
        let config_path = restore_dir.join("config.json");
        let config_str = tokio::fs::read_to_string(&config_path)
            .await
            .context("Reading snapshot config.json")?;

        let mut config: serde_json::Value =
            serde_json::from_str(&config_str).context("Parsing snapshot config.json")?;

        // Patch disk path
        if let Some(disks) = config.get_mut("disks").and_then(|d| d.as_array_mut()) {
            for disk in disks.iter_mut() {
                if let Some(path) = disk.get_mut("path") {
                    let old_path = path.as_str().unwrap_or("").to_string();
                    *path = serde_json::Value::String(new_disk_path.to_string_lossy().to_string());
                    tracing::debug!(
                        old_path = %old_path,
                        new_path = %new_disk_path.display(),
                        "Patched disk path in snapshot config"
                    );
                }
            }
        }

        // Patch kernel path
        if let Some(payload) = config.get_mut("payload") {
            if let Some(kernel) = payload.get_mut("kernel") {
                let old_path = kernel.as_str().unwrap_or("").to_string();
                *kernel = serde_json::Value::String(new_kernel_path.to_string_lossy().to_string());
                tracing::debug!(
                    old_path = %old_path,
                    new_path = %new_kernel_path.display(),
                    "Patched kernel path in snapshot config"
                );
            }
        }

        // Patch vsock socket path
        if let Some(vsock) = config.get_mut("vsock") {
            if let Some(socket) = vsock.get_mut("socket") {
                let old_path = socket.as_str().unwrap_or("").to_string();
                *socket =
                    serde_json::Value::String(new_vsock_socket_path.to_string_lossy().to_string());
                tracing::debug!(
                    old_path = %old_path,
                    new_path = %new_vsock_socket_path.display(),
                    "Patched vsock socket path in snapshot config"
                );
            }
        }

        let patched_config =
            serde_json::to_string_pretty(&config).context("Serializing patched config.json")?;
        tokio::fs::write(&config_path, patched_config)
            .await
            .context("Writing patched config.json")?;

        Ok(())
    }

    /// Common setup for restoring a Cloud Hypervisor VM from a snapshot.
    /// Copies kernel, copies device node, extracts archive, patches config, deletes tap.
    async fn prepare_ch_restore(
        vm_id: &Uuid,
        vm_config: &VmConfig,
        vm_drive_path: &Path,
        vm_network: &VmNetwork,
        archive_path: &Path,
        snapshot_id: &str,
    ) -> anyhow::Result<PathBuf> {
        let jail_path = get_ch_jail_root(vm_id);

        // Copy the drive device node to the jail
        let vm_root_drive_path = VersConfig::chelsea().vm_root_drive_path.clone();
        let dest_path = jail_path.join(&vm_root_drive_path);
        copy_device_node(vm_drive_path, &dest_path)
            .await
            .context("Copying VM drive device node")?;
        chown_vm(dest_path).context("chown device node")?;

        // Copy the kernel to the new jail root so the snapshot's kernel path can be patched to it
        let data_dir = DataDir::global();
        let kernel_src = data_dir.kernel_dir.join(&vm_config.kernel_name);
        let kernel_dest = jail_path.join("kernels").join(&vm_config.kernel_name);
        if let Some(parent) = kernel_dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating kernel parent dir for restore")?;
        }
        tokio::fs::copy(&kernel_src, &kernel_dest)
            .await
            .with_context(|| {
                format!("Copying kernel from {:?} to {:?}", kernel_src, kernel_dest)
            })?;
        chown_vm(&kernel_dest).context("Kernel chown for restore")?;

        // Extract the CH snapshot tar archive
        let restore_dir = jail_path.join(format!("snapshot-{}", snapshot_id));
        tokio::fs::create_dir_all(&restore_dir)
            .await
            .context("Creating restore directory")?;

        let status = tokio::process::Command::new("tar")
            .arg("xf")
            .arg(archive_path)
            .arg("-C")
            .arg(&restore_dir)
            .status()
            .await
            .context("Running tar to extract CH snapshot")?;

        if !status.success() {
            anyhow::bail!(
                "tar failed to extract CH snapshot (exit code: {:?})",
                status.code()
            );
        }

        // Patch the snapshot config.json with new paths
        let vsock_socket_path = jail_path.join("run/vsock.sock");
        if let Some(parent) = vsock_socket_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Creating vsock socket directory for restore")?;
        }
        Self::patch_ch_snapshot_config(
            &restore_dir,
            vm_drive_path,
            &kernel_dest,
            &vsock_socket_path,
        )
        .await
        .context("Patching CH snapshot config.json")?;

        // Delete the tap device before restore — CH will recreate it from the snapshot config.
        // If we don't delete it, CH fails with EBUSY.
        tracing::debug!(
            vm_id = %vm_id,
            netns = %vm_network.netns_name,
            tap_name = %vm_network.tap_name(),
            "Deleting tap device before Cloud Hypervisor restore"
        );

        let tap_del_result = netns_exec(
            &vm_network.netns_name,
            &["ip", "link", "del", &vm_network.tap_name()],
        )
        .await;

        match tap_del_result {
            Ok(_) => tracing::debug!(tap_name = %vm_network.tap_name(), "Tap device deleted"),
            Err(e) => tracing::debug!(
                tap_name = %vm_network.tap_name(),
                error = %e,
                "Tap device deletion failed (may not exist, proceeding)"
            ),
        }

        Ok(restore_dir)
    }

    async fn spawn_from_commit_cloud_hypervisor(
        &self,
        vm_id: Uuid,
        vm_config: &VmConfig,
        vm_commit_metadata: &VmCommitMetadata,
        vm_network: &VmNetwork,
        vm_drive_path: &Path,
        committed_ssh_private_key: &PrivateKey,
    ) -> anyhow::Result<Arc<VmProcess>> {
        let commit_id = vm_commit_metadata.commit_id;

        tracing::info!(
            vm_id = %vm_id,
            commit_id = %commit_id,
            netns = %vm_network.netns_name,
            "Starting Cloud Hypervisor restore from commit"
        );

        // Set up jail directory
        let jail_path = get_ch_jail_root(&vm_id);
        tokio::fs::create_dir_all(&jail_path)
            .await
            .context("Creating jail root directory")?;

        // Defer cleaning up the jail root
        let mut defer = DeferAsync::new();
        defer.defer({
            let jail_path = jail_path.clone();
            async move {
                if let Err(error) = tokio::fs::remove_dir_all(jail_path).await {
                    error!(%error, "Error cleaning up jail root for Cloud Hypervisor commit restore");
                }
            }
        });

        // Locate the snapshot archive
        let commit_id_str = vm_commit_metadata.commit_id.to_string();
        let commit_filepaths =
            VmCloudHypervisorProcessCommitFilepaths::from_commit_id(&commit_id_str).await?;
        let archive_path = &commit_filepaths.snapshot_tar_path;

        // Prepare for restore: copy device node, kernel, extract archive, patch config, delete tap
        let restore_dir = Self::prepare_ch_restore(
            &vm_id,
            vm_config,
            vm_drive_path,
            vm_network,
            archive_path,
            &commit_id_str,
        )
        .await?;

        // Create stdout and stderr logs
        let data_dir = DataDir::global();
        let (stdout_file, stderr_file) = data_dir
            .create_process_logs(&vm_id)
            .await
            .context("Creating process logs")?;

        // Spawn cloud-hypervisor process
        tracing::info!(vm_id = %vm_id, "Spawning cloud-hypervisor process for snapshot restore");
        let ch_process = CloudHypervisorProcess::new(
            vm_id.clone(),
            stdout_file.into_std().await,
            stderr_file.into_std().await,
            &vm_network.netns_name,
        )
        .await?;

        let source_url = format!("file://{}", restore_dir.display());

        tracing::info!(
            vm_id = %vm_id,
            commit_id = %commit_id,
            source_url = %source_url,
            "Calling Cloud Hypervisor API: vm.restore"
        );

        ch_process
            .api
            .restore_vm(&VmRestoreConfig {
                source_url,
                prefault: Some(false),
            })
            .await
            .context("Cloud Hypervisor vm.restore API call")?;

        tracing::info!(vm_id = %vm_id, commit_id = %commit_id, "Cloud Hypervisor VM restored successfully");

        // Configure the tap device that CH created during restore.
        // CH creates a bare tap without IP configuration, so we need to add IPs.
        use crate::network::utils::ipv4_to_mac;
        use crate::network::{TAP_NAME, TAP_NET_V4, TAP_NET_V6};

        let tap_mac = ipv4_to_mac(&TAP_NET_V4.addr());
        let netns = &vm_network.netns_name;

        // Add IPv4 and IPv6 addresses to the tap
        netns_exec(
            netns,
            &[
                "ip",
                "addr",
                "add",
                &TAP_NET_V4.to_string(),
                "dev",
                TAP_NAME,
            ],
        )
        .await
        .context("Setting tap IPv4 after restore")?;
        netns_exec(
            netns,
            &[
                "ip",
                "addr",
                "add",
                &TAP_NET_V6.to_string(),
                "dev",
                TAP_NAME,
            ],
        )
        .await
        .context("Setting tap IPv6 after restore")?;
        netns_exec(
            netns,
            &[
                "ip",
                "link",
                "set",
                "dev",
                TAP_NAME,
                "address",
                &tap_mac.to_string(),
            ],
        )
        .await
        .context("Setting tap MAC after restore")?;

        tracing::debug!(vm_id = %vm_id, "Tap device configured after restore");

        // Resume the restored VM
        ch_process
            .api
            .resume_vm()
            .await
            .context("Cloud Hypervisor vm.resume after restore")?;

        tracing::info!(vm_id = %vm_id, commit_id = %commit_id, "Cloud Hypervisor VM resumed successfully");

        // Wrap in Arc<VmProcess> for return
        let process = Arc::new(VmProcess::CloudHypervisor(ch_process));

        // Defer cleaning up the process
        defer.defer({
            let process = process.clone();
            async move {
                if let Err(error) = process.kill().await {
                    error!(%error, "Error cleaning up Cloud Hypervisor process");
                }
            }
        });

        // Install new SSH key via the committed credentials
        let new_public_key = PublicKey::from(vm_config.ssh_keypair.public.clone())
            .to_openssh()?
            .replace('\'', "'\\''");
        let install_key_cmd = format!(
            "mkdir -p /root/.ssh && chmod 700 /root/.ssh && printf '%s\\n' '{pub}' > /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys",
            pub = new_public_key,
        );

        let vm_host = vm_network.vm_addr.to_string();

        // Retry SSH with exponential backoff
        let mut last_error = None;
        for attempt in 1..=10 {
            match exec_ssh(committed_ssh_private_key, &vm_host, &install_key_cmd).await {
                Ok(_) => {
                    tracing::info!(vm_id = %vm_id, attempt = attempt, "SSH key installation succeeded");
                    last_error = None;
                    break;
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < 10 {
                        let delay_ms = std::cmp::min(100 * (1 << attempt), 5000);
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        if let Some(err) = last_error {
            return Err(err).context("Installing new SSH public key after 10 retries");
        }

        // Write VmMetadata to /etc/vminfo and ID to /etc/vm_id via vsock
        write_vm_metadata_via_vsock(&process.vm_id(), HypervisorType::CloudHypervisor).await?;

        defer.commit();
        Ok(process)
    }

    /// Restores a CloudHypervisor VM from a sleep snapshot
    async fn restore_process_cloud_hypervisor(
        &self,
        vm_id: Uuid,
        vm_config: &VmConfig,
        vm_network: &VmNetwork,
        vm_drive_path: &Path,
    ) -> anyhow::Result<Arc<VmProcess>> {
        tracing::info!(
            vm_id = %vm_id,
            netns = %vm_network.netns_name,
            "Starting Cloud Hypervisor restore from sleep snapshot"
        );

        // Set up jail directory
        let jail_path = get_ch_jail_root(&vm_id);
        tokio::fs::create_dir_all(&jail_path)
            .await
            .context("Creating jail root directory")?;

        // Defer cleaning up the jail root
        let mut defer = DeferAsync::new();
        defer.defer({
            let jail_path = jail_path.clone();
            async move {
                if let Err(error) = tokio::fs::remove_dir_all(jail_path).await {
                    error!(%error, "Error cleaning up jail root for Cloud Hypervisor sleep snapshot restore");
                }
            }
        });

        // Locate the snapshot archive
        let vm_id_str = vm_id.to_string();
        let sleep_snapshot_filepaths =
            VmCloudHypervisorProcessSleepSnapshotFilepaths::from_vm_id(&vm_id_str).await?;
        let archive_path = &sleep_snapshot_filepaths.snapshot_tar_path;

        // Prepare for restore: copy device node, kernel, extract archive, patch config, delete tap
        let restore_dir = Self::prepare_ch_restore(
            &vm_id,
            vm_config,
            vm_drive_path,
            vm_network,
            archive_path,
            &vm_id_str,
        )
        .await?;

        // Create stdout and stderr logs
        let data_dir = DataDir::global();
        let (stdout_file, stderr_file) = data_dir
            .create_process_logs(&vm_id)
            .await
            .context("Creating process logs")?;

        // Spawn cloud-hypervisor process
        tracing::info!(vm_id = %vm_id, "Spawning cloud-hypervisor process for sleep snapshot restore");
        let ch_process = CloudHypervisorProcess::new(
            vm_id.clone(),
            stdout_file.into_std().await,
            stderr_file.into_std().await,
            &vm_network.netns_name,
        )
        .await?;

        let source_url = format!("file://{}", restore_dir.display());

        tracing::info!(
            vm_id = %vm_id,
            source_url = %source_url,
            "Calling Cloud Hypervisor API: vm.restore"
        );

        ch_process
            .api
            .restore_vm(&VmRestoreConfig {
                source_url,
                prefault: Some(false),
            })
            .await
            .context("Cloud Hypervisor vm.restore API call")?;

        tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM restored from sleep snapshot");

        // Configure the tap device that CH created during restore.
        // CH creates a bare tap without IP configuration, so we need to add IPs.
        use crate::network::utils::ipv4_to_mac;
        use crate::network::{TAP_NAME, TAP_NET_V4, TAP_NET_V6};

        let tap_mac = ipv4_to_mac(&TAP_NET_V4.addr());
        let netns = &vm_network.netns_name;

        // Add IPv4 and IPv6 addresses to the tap
        netns_exec(
            netns,
            &[
                "ip",
                "addr",
                "add",
                &TAP_NET_V4.to_string(),
                "dev",
                TAP_NAME,
            ],
        )
        .await
        .context("Setting tap IPv4 after sleep restore")?;
        netns_exec(
            netns,
            &[
                "ip",
                "addr",
                "add",
                &TAP_NET_V6.to_string(),
                "dev",
                TAP_NAME,
            ],
        )
        .await
        .context("Setting tap IPv6 after sleep restore")?;
        netns_exec(
            netns,
            &[
                "ip",
                "link",
                "set",
                "dev",
                TAP_NAME,
                "address",
                &tap_mac.to_string(),
            ],
        )
        .await
        .context("Setting tap MAC after sleep restore")?;

        tracing::debug!(vm_id = %vm_id, "Tap device configured after sleep restore");

        // Resume the restored VM
        ch_process
            .api
            .resume_vm()
            .await
            .context("Cloud Hypervisor vm.resume after restore")?;

        tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM resumed successfully");

        // Wrap in Arc<VmProcess> for return
        let process = Arc::new(VmProcess::CloudHypervisor(ch_process));

        // Defer cleaning up the process
        defer.defer({
            let process = process.clone();
            async move {
                if let Err(error) = process.kill().await {
                    error!(%error, "Error cleaning up Cloud Hypervisor process");
                }
            }
        });

        // Write VmMetadata to /etc/vminfo and ID to /etc/vm_id via vsock
        write_vm_metadata_via_vsock(&process.vm_id(), HypervisorType::CloudHypervisor).await?;

        defer.commit();
        Ok(process)
    }

    /// Spawns a CloudHypervisor VM from a sleep snapshot
    async fn spawn_from_sleep_snapshot_cloud_hypervisor(
        &self,
        vm_id: Uuid,
        vm_config: &VmConfig,
        _vm_process_sleep_snapshot_metadata: &vers_pg::schema::chelsea::tables::sleep_snapshot::RecordCloudHypervisorProcessSleepSnapshot,
        vm_network: &VmNetwork,
        vm_drive_path: &Path,
    ) -> anyhow::Result<Arc<VmProcess>> {
        // Delegate to restore_process_cloud_hypervisor which handles the actual restore
        self.restore_process_cloud_hypervisor(vm_id, vm_config, vm_network, vm_drive_path)
            .await
    }
}
