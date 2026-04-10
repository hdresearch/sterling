use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    commit_store::VmCommitStore,
    network_manager::{manager::VmNetworkManager, store::VmNetworkRecord},
    process::cloud_hypervisor::config::{
        CloudHypervisorProcessConfig, CloudHypervisorProcessVsockConfig,
    },
    process::firecracker::config::{FirecrackerProcessConfig, FirecrackerProcessVsockConfig},
    process_manager::{VmProcessCommitMetadata, VmProcessConfig, VmProcessManager},
    ready_service::{
        VmBootSubscribeResult, VmReadyService,
        error::{VmBootError, VmReadyServiceError},
    },
    sleep_snapshot_store::VmSleepSnapshotStore,
    system_service::SystemService,
    vm::{Vm, VmConfig, VmWireGuardConfig},
    vm_manager::{
        commit::{VmCommitMetadata, VmConfigCommit},
        error::{
            VmAllocationError, VmAllocationType, VmLifecycleError, VmLookupError, VmManagerError,
        },
        store::VmManagerStore,
        types::{VmEvent, VmState, VmSummary},
    },
    volume_manager::{VmVolumeCommitMetadata, VmVolumeManager},
    vsock::VsockClient,
};
use anyhow::{Context, anyhow};
use ssh_key::{PrivateKey, private::KeypairData};
use ssh_key::{private::Ed25519Keypair, rand_core::OsRng};
use tracing::{Instrument, debug, error, warn};
use util::{defer::DeferAsync, exec_ssh, join_errors, linux::get_host_cpu_architecture};
use uuid::Uuid;
use vers_config::{HypervisorType, VersConfig};
use vers_pg::{
    db::VersPg,
    schema::chelsea::tables::{
        commit::CommitFile,
        sleep_snapshot::RecordSleepSnapshot,
        vm::{RecordCephVmVolume, RecordVm, RecordVmVolume},
        vm_usage_segment::RecordVmUsageSegmentStart,
    },
};

/// Resolve the host-side vsock socket path for a given VM.
/// Returns the appropriate path based on the hypervisor type.
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

/// Handle the bookkeeping associated with a VM that just finished booting.
///
/// This helper is shared between the vsock ready monitor and the legacy HTTP
/// `notify-ready` callback. Both paths race — whichever fires first marks the
/// VM as booted and the other receives a harmless `NotFound` error (the VM is
/// no longer in the "booting" map).
async fn complete_vm_boot(ready_service: &VmReadyService, vm_id: &Uuid, source: &'static str) {
    match ready_service.remove_booting_vm(vm_id, Ok(())).await {
        Ok(()) => {
            debug!(vm_id = %vm_id, ready_source = source, "VM marked as ready");
        }
        Err(VmReadyServiceError::NotFound(_)) => {
            // The other path already completed boot — this is expected.
            debug!(
                vm_id = %vm_id,
                ready_source = source,
                "VM already marked ready via alternate path"
            );
        }
        Err(error) => {
            error!(vm_id = %vm_id, ready_source = source, %error, "Failed to mark VM ready");
        }
    }
}

/// Background task: wait for the in-VM vsock agent to send a Ready event.
///
/// If the VM has no agent (old base image), the vsock connection will fail and
/// this task logs a warning and exits — the legacy HTTP `notify-ready` path
/// will handle boot detection instead.
async fn wait_for_vsock_agent_ready(
    vm_id: Uuid,
    hypervisor_type: HypervisorType,
    ready_service: Arc<VmReadyService>,
) {
    let socket_path = dbg!(vsock_socket_path(&vm_id, hypervisor_type));
    let client = VsockClient::new(&socket_path);
    let timeout = Duration::from_secs(VersConfig::chelsea().vm_boot_timeout_secs);

    debug!(
        vm_id = %vm_id,
        socket = %socket_path.display(),
        "Waiting for vsock agent to report ready"
    );

    match client.wait_ready(timeout).await {
        Ok(ready) => {
            debug!(
                vm_id = %vm_id,
                version = ready.version,
                capabilities = ?ready.capabilities,
                "Vsock agent reported ready"
            );
            complete_vm_boot(&ready_service, &vm_id, "vsock").await;
        }
        Err(error) => {
            // This is expected for old VMs without the agent — the HTTP
            // notify-ready path will handle boot detection instead.
            warn!(
                vm_id = %vm_id,
                %error,
                "Vsock readiness monitor failed (expected for VMs without agent)"
            );
        }
    }
}

/// Path where user-provided env vars are written inside the guest.
/// `/etc/environment` is read by PAM for all login sessions (SSH, etc.)
/// and by the chelsea-agent on startup so exec'd processes inherit them.
const VM_ENV_PATH: &str = "/etc/environment";

/// Render a HashMap of env vars into `/etc/environment` format.
///
/// The format is simple `KEY=VALUE` lines — no `export`, no shell quoting.
/// PAM's `pam_env` module parses this file directly; it is not evaluated by
/// a shell. Output is sorted by key for deterministic diffs.
fn render_env_file(env_vars: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = env_vars.keys().collect();
    keys.sort_unstable();

    let mut content = String::new();
    for key in keys {
        if let Some(value) = env_vars.get(key) {
            content.push_str(key);
            content.push('=');
            content.push_str(value);
            content.push('\n');
        }
    }

    content
}

/// Wait for the vsock agent to be ready, then write env vars via WriteFile.
async fn write_env_vars_to_vm(
    vm_id: Uuid,
    env_vars: HashMap<String, String>,
    hypervisor_type: HypervisorType,
) -> anyhow::Result<()> {
    if env_vars.is_empty() {
        return Ok(());
    }

    let socket_path = vsock_socket_path(&vm_id, hypervisor_type);
    let client = VsockClient::new(&socket_path);
    let timeout = Duration::from_secs(VersConfig::chelsea().vm_boot_timeout_secs);

    client
        .wait_ready(timeout)
        .await
        .map_err(|error| anyhow!(error))?;

    let payload = render_env_file(&env_vars);
    client
        .write_file(VM_ENV_PATH, payload.as_bytes(), 0o644, true)
        .await
        .map_err(|error| anyhow!(error))?;

    debug!(vm_id = %vm_id, num_vars = env_vars.len(), "Wrote user env vars to VM");
    Ok(())
}

/// Fire-and-forget task to install env vars inside a VM after it boots.
fn spawn_env_writer(
    vm_id: Uuid,
    env_vars: HashMap<String, String>,
    hypervisor_type: HypervisorType,
) {
    tokio::spawn(async move {
        if let Err(error) = write_env_vars_to_vm(vm_id, env_vars, hypervisor_type).await {
            warn!(%vm_id, %error, "Failed to install user env vars inside VM");
        }
    });
}

/// A high-level interface to spawning VMs
pub struct VmManager {
    /// The local data store
    pub local_store: Arc<dyn VmManagerStore>,
    /// The remote data store
    pub remote_store: Arc<VersPg>,
    pub network_manager: Arc<VmNetworkManager>,
    pub process_manager: Arc<VmProcessManager>,
    pub system_service: Arc<SystemService>,
    pub volume_manager: Arc<dyn VmVolumeManager>,
    pub commit_store: Arc<dyn VmCommitStore>,
    pub sleep_snapshot_store: Arc<dyn VmSleepSnapshotStore>,
    pub ready_service: Arc<VmReadyService>,
    /// Per-VM mutex to serialize lifecycle operations (resize, commit, sleep, etc.)
    pub vm_locks: std::sync::Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>,
    /// The hypervisor type used for all VMs on this node
    pub hypervisor_type: HypervisorType,
}

impl VmManager {
    /// Returns the per-VM mutex for serializing lifecycle operations.
    fn vm_lock(&self, vm_id: &Uuid) -> Result<Arc<tokio::sync::Mutex<()>>, VmManagerError> {
        let mut map = self
            .vm_locks
            .lock()
            .map_err(|e| VmManagerError::Other(anyhow!("vm_locks mutex poisoned: {e}")))?;
        Ok(map
            .entry(*vm_id)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone())
    }

    /// Rehydrate a VM using information from data aggregated from the VmManagers's managers + services. Use with caution; direct access to this object's members (process, network, volume)
    /// will not cause corresponding updates to their state as tracked by their respective manager.
    async fn rehydrate_vm(&self, vm_id: &Uuid) -> Result<Vm, VmManagerError> {
        // Fetch VM config from VmManager data
        let vm_record = self
            .local_store
            .fetch_vm_record(vm_id)
            .await?
            .ok_or_else(|| {
                VmManagerError::Other(anyhow!(VmLookupError::Vm {
                    vm_id: vm_id.to_string()
                }))
            })?;

        let ssh_private_key = PrivateKey::from_openssh(vm_record.ssh_private_key)
            .map_err(|e| VmManagerError::Other(anyhow!(e)))?;
        let ssh_keypair = match ssh_private_key.key_data() {
            KeypairData::Ed25519(ssh_keypair) => ssh_keypair,
            _ => {
                return Err(VmManagerError::Other(anyhow!(
                    "Unable to interpret private key data from VM {vm_id} as Ed25519"
                )));
            }
        }
        .clone();

        // Rehydrate process struct from ProcessManager data (needed for hypervisor_type)
        let process = self
            .process_manager
            .rehydrate_vm_process(vm_record.vm_process_pid)
            .await?;

        let config = VmConfig {
            kernel_name: vm_record.kernel_name,
            base_image: vm_record.image_name,
            vcpu_count: vm_record.vcpu_count,
            mem_size_mib: vm_record.mem_size_mib,
            fs_size_mib: vm_record.fs_size_mib,
            ssh_keypair,
            hypervisor_type: process.process_type(),
        };

        // Rehydrate network struct from NetworkManager data
        let network = self
            .network_manager
            .rehydrate_network(&vm_record.vm_network_host_addr)
            .await?;

        // Rehydrate volume struct from VolumeManager data
        let volume = self
            .volume_manager
            .rehydrate_vm_volume(&vm_record.vm_volume_id)
            .await?;

        Ok(Vm {
            id: vm_record.id,
            config,
            process,
            network,
            volume,
        })
    }

    /// Creates a new VM.
    #[tracing::instrument(skip_all, fields(%vm_id, wait_boot))]
    pub async fn create_new_vm(
        &self,
        vm_id: Uuid,
        vm_config: VmConfig,
        wg: VmWireGuardConfig,
        wait_boot: bool,
        env_vars: Option<HashMap<String, String>>,
    ) -> Result<(), VmManagerError> {
        // Check that the requested VM would not exceed configured maxima or available resources
        self.check_vm_reservation(
            vm_config.vcpu_count,
            vm_config.mem_size_mib,
            vm_config.fs_size_mib,
        )
        .instrument(tracing::info_span!("chelsea.check_vm_reservation"))
        .await?;

        // Create the network for the VM
        let network = self
            .network_manager
            .reserve_network()
            .instrument(tracing::info_span!("chelsea.reserve_network"))
            .await?;

        // Defer deleting the network
        let mut defer = DeferAsync::new();
        defer.defer({
            let network_manager = self.network_manager.clone();
            let host_addr = network.host_addr.clone();
            async move {
                if let Err(error) = network_manager.on_vm_killed(&host_addr).await {
                    error!(
                        %error,
                        ?host_addr,
                        "Error while cleaning up VM network via manager"
                    );
                    if let Err(unreserve_error) =
                        network_manager.release_reserved_network(&host_addr).await
                    {
                        error!(
                            %unreserve_error,
                            ?host_addr,
                            "Fallback network release also failed while cleaning up VM network"
                        );
                    }
                }
            }
        });

        // Create a new volume for the VM.
        let volume = self
            .volume_manager
            .create_volume_from_base_image(vm_config.base_image.clone(), vm_config.fs_size_mib)
            .instrument(tracing::info_span!("chelsea.create_volume"))
            .await?;

        // Defer deleting the volume.
        defer.defer({
            let volume_manager = self.volume_manager.clone();
            let volume_clone = volume.clone();
            let volume_id = volume_clone.id();

            async move {
                if let Err(error) = volume_manager.on_vm_killed(&volume_id).await {
                    error!(
                        %error,
                        vm_volume_id = %volume_id,
                        "Error while cleaning up VM volume via manager"
                    );
                    if let Err(delete_error) = volume_clone.delete().await {
                        error!(
                            %delete_error,
                            vm_volume_id = %volume_id,
                            "Fallback volume delete also failed while cleaning up VM volume"
                        );
                    }
                }
            }
        });

        // Inform the readiness service that the VM is booting.
        self.ready_service.insert_booting_vm(vm_id.clone()).await;

        // Defer informing the readiness service that VM boot has been aborted.
        defer.defer({
            let ready_service = self.ready_service.clone();
            let vm_id = vm_id.clone();
            async move {
                if let Err(error) = ready_service
                    .remove_booting_vm(&vm_id, Err(VmBootError::Aborted))
                    .await
                {
                    warn!(%error, "Error while removing cleaned up VM from ReadyService");
                }
            }
        });

        // Spawn process - select config based on hypervisor type
        let process_config = match vm_config.hypervisor_type {
            HypervisorType::Firecracker => {
                let config = FirecrackerProcessConfig::with_defaults(
                    vm_id.clone(),
                    &volume,
                    &network,
                    &vm_config,
                    &self.ready_service.chelsea_notify_boot_url_template,
                );
                VmProcessConfig::Firecracker(config)
            }
            HypervisorType::CloudHypervisor => {
                let config = CloudHypervisorProcessConfig::with_defaults(
                    vm_id.clone(),
                    &volume,
                    &network,
                    &vm_config,
                    &self.ready_service.chelsea_notify_boot_url_template,
                )
                .await;
                VmProcessConfig::CloudHypervisor(config)
            }
        };

        tracing::info!(
            vm_id = %vm_id,
            hypervisor = %vm_config.hypervisor_type.to_string(),
            "Spawning VM with hypervisor"
        );

        // Ensure TAP device exists in the network namespace before spawning.
        // This handles the case where TAP was deleted when a previous VM exited
        // but the namespace is being reused for a new VM.
        network.ensure_tap().await?;

        let process = self
            .process_manager
            .spawn_new(
                vm_id.clone(),
                &vm_config,
                &process_config,
                &network.netns_name,
            )
            .instrument(tracing::info_span!("chelsea.spawn_process"))
            .await?;

        // Spawn vsock readiness monitor — races with HTTP notify-ready.
        // For old VMs without the agent, this will fail gracefully and the
        // HTTP path handles boot detection.
        tokio::spawn(wait_for_vsock_agent_ready(
            vm_id.clone(),
            vm_config.hypervisor_type,
            self.ready_service.clone(),
        ));

        // Spawn env var writer if user provided any (waits for agent ready then writes)
        if let Some(env_vars) = env_vars {
            if !env_vars.is_empty() {
                spawn_env_writer(vm_id.clone(), env_vars, self.hypervisor_type);
            }
        }

        // Defer cleaning up VM process
        let process_pid = process.pid().await?;
        let process_manager = self.process_manager.clone();
        let process_clone = process.clone();
        defer.defer(async move {
            if let Err(error) = process_manager.kill(process_pid).await {
                error!(
                    %error,
                    vm_process_pid = process_pid,
                    "Error while cleaning up VM process via manager"
                );
                if let Err(kill_error) = process_clone.kill().await {
                    error!(
                        %kill_error,
                        vm_process_pid = process_pid,
                        "Fallback process kill also failed while cleaning up VM process"
                    );
                }
            }
        });

        let vm = Vm::new(vm_id.clone(), vm_config, process, network, volume);

        // Inform the network manager that a VM has had Wireguard config attached to it.
        self.network_manager
            .on_vm_created(&vm.network.host_addr, wg)
            .instrument(tracing::info_span!("chelsea.wg_setup"))
            .await?;

        // Configure and setup WireGuard if config is present
        // vm.network.configure_wireguard(wg)?;

        // Insert VM record into local store
        async {
            let record = vm
                .as_record()
                .await
                .context("Interpreting new Vm as record")?;
            self.local_store
                .insert_vm_record(record)
                .await
                .context("Inserting new Vm into store")
        }
        .instrument(tracing::info_span!("chelsea.local_store_insert"))
        .await?;

        // Defer deleting VM record from local store
        defer.defer({
            let store = self.local_store.clone();
            let vm_id = vm_id.clone();
            async move {
                if let Err(error) = store.delete_vm_record(&vm_id).await {
                    error!(
                        %error,
                        %vm_id,
                        "Error while cleaning up VM record from store"
                    );
                }
            }
        });

        // Insert VM record into remote store
        // TODO: Don't hardcode Ceph
        self.remote_store
            .chelsea
            .vm
            .insert(&RecordVm {
                id: vm.id,
                volume: RecordVmVolume::Ceph(RecordCephVmVolume {
                    image_name: vm.volume.image_name(),
                }),
            })
            .instrument(tracing::info_span!("chelsea.remote_store_vm_insert"))
            .await?;

        // Defer deleting VM record from remote store
        defer.defer({
            let remote_store = self.remote_store.clone();
            let vm_id = vm_id.clone();
            async move {
                if let Err(error) = remote_store.chelsea.vm.delete_by_id(&vm_id).await {
                    error!(
                        %error,
                        %vm_id,
                        "Error while cleaning up VM record from remote store"
                    );
                }
            }
        });

        // Insert VM usage start event into remote store
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        self.remote_store
            .chelsea
            .vm_usage_segment
            .insert_start(&RecordVmUsageSegmentStart {
                vm_id: vm_id.clone(),
                start_timestamp: now,
                start_created_at: now,
                vcpu_count: vm.config.vcpu_count,
                ram_mib: vm.config.mem_size_mib,
                disk_gib: None,
                start_code: None,
            })
            .instrument(tracing::info_span!("chelsea.remote_store_usage_insert"))
            .await?;

        // Defer updating the VM usage segment with stop metadata
        defer.defer({
            let remote_store = self.remote_store.clone();
            let vm_id = vm_id.clone();
            async move {
                let now = match SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs() as i64) {
                    Ok(duration) => duration,
                    Err(error) => {
                        error!(%error, "Error getting system time while attempting to record VM stop metadata");
                        return;
                    }
                };
                match remote_store
                    .chelsea
                    .vm_usage_segment
                    .complete_latest_segment(&vm_id, now, now, None)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        warn!(
                            %vm_id,
                            "No open VM usage segment found while attempting to record stop metadata"
                        );
                    }
                    Err(error) => {
                        error!(
                            %error,
                            %vm_id,
                            "Error while updating VM usage segment stop metadata in remote store"
                        );
                    }
                }
            }
        });

        // Spawn a task to await booting, cleaning up (via defer.cleanup()) if booting fails
        let _boot_task = tokio::spawn({
            let ready_service = self.ready_service.clone();
            let vm_id = vm_id.clone();

            async move {
                match ready_service.wait_vm_boot(&vm_id).await {
                    Ok(()) => {
                        defer.commit();
                        Ok(())
                    }
                    Err(error) => {
                        error!(%error, %vm_id, "VM failed to boot");
                        defer.cleanup().await;
                        notify_orchestrator_boot_failed(&vm_id).await;
                        Err(error)
                    }
                }
            }
        });

        // Optionally wait for the VM to finish booting
        if wait_boot {
            self.ready_service
                .wait_vm_boot(&vm_id)
                .instrument(tracing::info_span!("chelsea.wait_vm_boot"))
                .await?;
        }

        Ok(())
    }

    /// Rehydrate a VmCommitMetadata from the remote store
    pub async fn rehydrate_commit_metadata(
        &self,
        commit_id: &Uuid,
    ) -> Result<VmCommitMetadata, VmManagerError> {
        let commit_record = self
            .remote_store
            .chelsea
            .commit
            .fetch_by_id(commit_id)
            .await?;

        Ok(VmCommitMetadata {
            commit_id: commit_record.id,
            host_architecture: commit_record.host_architecture,
            process_metadata: VmProcessCommitMetadata::from(commit_record.process_commit),
            volume_metadata: VmVolumeCommitMetadata::try_from(commit_record.volume_commit)
                .map_err(|e| VmManagerError::Other(anyhow!(e)))?,
            vm_config: VmConfigCommit {
                kernel_name: commit_record.kernel_name,
                base_image: commit_record.base_image,
                vcpu_count: commit_record.vcpu_count,
                mem_size_mib: commit_record.mem_size_mib,
                fs_size_mib: commit_record.fs_size_mib,
                ssh_public_key: commit_record.ssh_public_key,
                ssh_private_key: commit_record.ssh_private_key,
            },
            remote_files: commit_record
                .remote_files
                .into_iter()
                .map(CommitFile::from)
                .collect(),
        })
    }

    /// Create a new VM, getting or downloading the commit files for commit ID.
    pub async fn create_vm_from_commit(
        &self,
        vm_id: Uuid,
        commit_id: &Uuid,
        wg: VmWireGuardConfig,
        env_vars: Option<HashMap<String, String>>,
    ) -> Result<(), VmManagerError> {
        // Fetch the VM metadata from the persistent store
        let vm_commit_metadata = self.rehydrate_commit_metadata(commit_id).await?;

        // Check that the requested VM size would not exceed configured maxima or current resource availability
        self.check_vm_reservation(
            vm_commit_metadata.vm_config.vcpu_count,
            vm_commit_metadata.vm_config.mem_size_mib,
            vm_commit_metadata.vm_config.fs_size_mib,
        )
        .await?;

        // Download all associated commit files to the commit data directory
        self.commit_store
            .download_commit_files(&vm_commit_metadata.remote_files)
            .await?;

        // Create the network for the VM
        let network = self.network_manager.reserve_network().await?;

        // Defer deleting the network
        let mut defer = DeferAsync::new();
        defer.defer({
            let network_manager = self.network_manager.clone();
            let host_addr = network.host_addr.clone();
            async move {
                if let Err(error) = network_manager.on_vm_killed(&host_addr).await {
                    error!(
                        %error,
                        ?host_addr,
                        "Error while cleaning up VM network via manager"
                    );
                    if let Err(unreserve_error) =
                        network_manager.release_reserved_network(&host_addr).await
                    {
                        error!(
                            %unreserve_error,
                            ?host_addr,
                            "Fallback network release also failed while cleaning up VM network"
                        );
                    }
                }
            }
        });

        // Create the new volume for the VM from the commit metadata.
        let volume = self
            .volume_manager
            .create_volume_from_commit_metadata(&vm_commit_metadata.volume_metadata)
            .await?;

        // Defer deleting the volume.
        defer.defer({
            let volume_manager = self.volume_manager.clone();
            let volume_clone = volume.clone();
            let volume_id = volume_clone.id();

            async move {
                if let Err(error) = volume_manager.on_vm_killed(&volume_id).await {
                    error!(
                        %error,
                        vm_volume_id = %volume_id,
                        "Error while cleaning up VM volume via manager"
                    );
                    if let Err(delete_error) = volume_clone.delete().await {
                        error!(
                            %delete_error,
                            vm_volume_id = %volume_id,
                            "Fallback volume delete also failed while cleaning up VM volume"
                        );
                    }
                }
            }
        });

        // Construct new VmConfig by replacing the SSH keypair in the committed config with a random one.
        // We will still need the committed SSH private key to install the newly-generated one later.
        let VmConfigCommit {
            kernel_name,
            base_image,
            vcpu_count,
            mem_size_mib,
            fs_size_mib,
            ssh_private_key: committed_ssh_private_key,
            ..
        } = vm_commit_metadata.vm_config.clone();

        let ssh_keypair = Ed25519Keypair::random(&mut OsRng);
        let committed_ssh_private_key =
            PrivateKey::from_openssh(committed_ssh_private_key.as_bytes())?;

        // Determine hypervisor type from commit metadata
        let hypervisor_type = match &vm_commit_metadata.process_metadata {
            VmProcessCommitMetadata::Firecracker(_) => HypervisorType::Firecracker,
            VmProcessCommitMetadata::CloudHypervisor(_) => HypervisorType::CloudHypervisor,
        };

        // Ensure TAP device exists (CH deletes it on exit, need to recreate)
        network.ensure_tap().await?;

        // Spawn process
        let process = self
            .process_manager
            .spawn_from_commit(
                vm_id.clone(),
                &VmConfig {
                    kernel_name: kernel_name.clone(),
                    base_image: base_image.clone(),
                    vcpu_count,
                    mem_size_mib,
                    fs_size_mib,
                    ssh_keypair: ssh_keypair.clone(),
                    hypervisor_type,
                },
                &vm_commit_metadata,
                &network,
                volume.path().as_path(),
                &committed_ssh_private_key,
            )
            .await?;

        // Spawn env var writer if user provided any
        if let Some(env_vars) = env_vars {
            if !env_vars.is_empty() {
                spawn_env_writer(vm_id.clone(), env_vars, self.hypervisor_type);
            }
        }

        // Defer cleaning up VM process
        let process_pid = process.pid().await?;
        let process_manager = self.process_manager.clone();
        let process_clone = process.clone();
        defer.defer(async move {
            if let Err(error) = process_manager.kill(process_pid).await {
                error!(
                    %error,
                    vm_process_pid = process_pid,
                    "Error while cleaning up VM process via manager"
                );
                if let Err(kill_error) = process_clone.kill().await {
                    error!(
                        %kill_error,
                        vm_process_pid = process_pid,
                        "Fallback process kill also failed while cleaning up VM process"
                    );
                }
            }
        });

        // Defer cleaning up network manager state (on_vm_killed)
        // IMPORTANT: Register this BEFORE calling on_vm_created so that if on_vm_created fails,
        // the cleanup will still be registered and the DeferAsync won't be dropped with pending tasks
        defer.defer({
            let network_manager = self.network_manager.clone();
            let host_addr = network.host_addr;
            async move {
                if let Err(error) = network_manager.on_vm_killed(&host_addr).await {
                    error!(
                        %error,
                        vm_network_host_addr = %host_addr,
                        "Error while cleaning up network manager state via on_vm_killed"
                    );
                }
            }
        });

        // Inform the network manager that a VM has had Wireguard config attached to it.
        // This can fail if the WireGuard interface/port is already in use
        self.network_manager
            .on_vm_created(&network.host_addr, wg)
            .await?;

        // Create VM struct
        let vm = Vm::new(
            vm_id.clone(),
            VmConfig {
                kernel_name,
                base_image,
                vcpu_count,
                mem_size_mib,
                fs_size_mib,
                ssh_keypair,
                hypervisor_type,
            },
            process,
            network,
            volume,
        );

        // Insert VM record into local store
        let record = vm
            .as_record()
            .await
            .context("Interpreting new Vm as record")?;
        self.local_store
            .insert_vm_record(record)
            .await
            .context("Inserting new Vm into store")?;

        // Defer deleting VM record from local store
        defer.defer({
            let store = self.local_store.clone();
            let vm_id = vm_id.clone();
            async move {
                if let Err(error) = store.delete_vm_record(&vm_id).await {
                    error!(
                        %error,
                        %vm_id,
                        "Error while cleaning up VM record from store"
                    );
                }
            }
        });

        // Insert VM record into remote store
        // TODO: Don't hardcode Ceph
        self.remote_store
            .chelsea
            .vm
            .insert(&RecordVm {
                id: vm.id,
                volume: RecordVmVolume::Ceph(RecordCephVmVolume {
                    image_name: vm.volume.image_name(),
                }),
            })
            .await?;

        // Defer deleting VM record from remote store
        defer.defer({
            let remote_store = self.remote_store.clone();
            let vm_id = vm_id.clone();
            async move {
                if let Err(error) = remote_store.chelsea.vm.delete_by_id(&vm_id).await {
                    error!(
                        %error,
                        %vm_id,
                        "Error while cleaning up VM record from remote store"
                    );
                }
            }
        });

        // Insert VM usage start event into remote store
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        self.remote_store
            .chelsea
            .vm_usage_segment
            .insert_start(&RecordVmUsageSegmentStart {
                vm_id: vm_id.clone(),
                start_timestamp: now,
                start_created_at: now,
                vcpu_count: vm.config.vcpu_count,
                ram_mib: vm.config.mem_size_mib,
                disk_gib: None,
                start_code: None,
            })
            .await?;

        defer.commit();
        Ok(())
    }

    /// Pause the specified VM. `wait_boot` specifies, if VM is booting, whether to wait for VM boot or return an error immediately.
    pub async fn pause_vm(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), VmManagerError> {
        // If the VM is booting, either wait or error immediately
        if let VmBootSubscribeResult::Booting(mut receiver) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            match wait_boot {
                false => {
                    return Err(VmLifecycleError::StillBooting {
                        vm_id: vm_id.to_string(),
                    }
                    .into());
                }
                true => receiver.recv().await??,
            };
        };

        let vm = self.rehydrate_vm(vm_id).await?;
        vm.process.pause().await.map_err(Into::into)
    }

    /// Resume the specified VM. `wait_boot` specifies, if VM is booting, whether to wait for VM boot or return an error immediately.
    pub async fn resume_vm(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), VmManagerError> {
        // If the VM is booting, either wait or error immediately
        if let VmBootSubscribeResult::Booting(mut receiver) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            match wait_boot {
                false => {
                    return Err(VmLifecycleError::StillBooting {
                        vm_id: vm_id.to_string(),
                    }
                    .into());
                }
                true => receiver.recv().await??,
            };
        };

        let vm = self
            .local_store
            .fetch_vm_record(vm_id)
            .await?
            .ok_or_else(|| {
                anyhow!(VmLookupError::Vm {
                    vm_id: vm_id.to_string()
                })
            })?;

        let (process_result, network_result, volume_result) = tokio::join!(
            self.process_manager.on_vm_resumed(vm.vm_process_pid),
            self.network_manager.on_vm_resumed(&vm.vm_network_host_addr),
            self.volume_manager.on_vm_resumed(&vm.vm_volume_id)
        );

        let errors = [process_result, network_result, volume_result]
            .into_iter()
            .filter_map(|result| result.err())
            .collect::<Vec<_>>();

        match errors.is_empty() {
            true => Ok(()),
            false => Err(VmManagerError::Other(anyhow!(join_errors(&errors, "; ")))),
        }
    }

    /// Resize a VM's disk. The VM is paused, the underlying block device is grown,
    /// the VM is resumed, and then resize2fs is run inside the guest via SSH.
    /// Only growing is supported; the new size must be strictly greater than the current size.
    /// `wait_boot` specifies, if VM is booting, whether to wait for VM boot or return an error immediately.
    pub async fn resize_vm_disk(
        &self,
        vm_id: &Uuid,
        new_fs_size_mib: u32,
        wait_boot: bool,
    ) -> Result<(), VmManagerError> {
        // If the VM is booting, either wait or error immediately
        if let VmBootSubscribeResult::Booting(mut receiver) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            match wait_boot {
                false => {
                    return Err(VmLifecycleError::StillBooting {
                        vm_id: vm_id.to_string(),
                    }
                    .into());
                }
                true => receiver.recv().await??,
            };
        };

        // Acquire per-VM lock to serialize lifecycle operations (resize, commit, sleep, etc.)
        let lock = self.vm_lock(vm_id)?;
        let _guard = lock.lock().await;

        let vm = self.rehydrate_vm(vm_id).await?;

        // Validate: new size must not exceed the hard maximum
        let max_volume_mib = VersConfig::chelsea().vm_max_volume_mib;
        if new_fs_size_mib > max_volume_mib {
            return Err(VmAllocationError::HardMaximumViolation {
                ty: VmAllocationType::Volume,
                requested: new_fs_size_mib,
                max: max_volume_mib,
            }
            .into());
        }

        // Validate: new size must be strictly greater than current size
        let current_fs_size_mib = vm.config.fs_size_mib;
        if new_fs_size_mib <= current_fs_size_mib {
            return Err(VmManagerError::Other(anyhow!(
                "New disk size ({} MiB) must be strictly greater than current size ({} MiB). Shrinking is not supported.",
                new_fs_size_mib,
                current_fs_size_mib
            )));
        }

        // Pause the VM if it is not already paused
        let was_running = !vm.process.is_paused().await?;
        if was_running {
            vm.process.pause().await?;
        }

        // Phase 1: Grow only the RBD block device (no filesystem changes).
        // The guest kernel owns the mounted filesystem, so fsck/resize2fs on the host
        // would corrupt metadata that the guest has cached in memory.
        let resize_result = self
            .volume_manager
            .resize_volume_device_only(&vm.volume.id(), new_fs_size_mib)
            .await;

        // If device resize failed, resume the VM before returning the error
        if let Err(error) = resize_result {
            if was_running {
                if let Err(resume_error) = vm.process.resume().await {
                    error!(%resume_error, %vm_id, "Failed to resume VM after block device resize failure");
                }
            }
            return Err(VmManagerError::Other(
                error.context("Failed to resize VM block device"),
            ));
        }

        // Notify hypervisor that the block device has changed size so the guest
        // kernel sees the larger device. The drive path is jail-relative.
        let drive_path = VersConfig::chelsea().vm_root_drive_path.clone();
        if let Err(error) = vm.process.update_drive("root", &drive_path).await {
            error!(%error, %vm_id, "Failed to notify hypervisor of drive resize");
            if was_running {
                if let Err(resume_error) = vm.process.resume().await {
                    error!(%resume_error, %vm_id, "Failed to resume VM after drive update failure");
                }
            }
            return Err(VmManagerError::Other(error.context(
                "Failed to update hypervisor drive after block device resize",
            )));
        }

        // Update fs_size_mib in the local store.
        // If this fails, the block device is already grown but the DB is stale.
        // Best-effort resume the VM so it isn't left paused.
        if let Err(error) = self
            .local_store
            .update_vm_fs_size_mib(vm_id, new_fs_size_mib)
            .await
        {
            error!(%error, %vm_id, "Failed to update fs_size_mib in DB after successful block device resize");
            if was_running {
                if let Err(resume_error) = vm.process.resume().await {
                    error!(%resume_error, %vm_id, "Failed to resume VM after DB update failure");
                }
            }
            return Err(VmManagerError::Other(
                anyhow::Error::from(error).context("Failed to update fs_size_mib in local store"),
            ));
        }

        // Resume the VM if it was running before
        if was_running {
            vm.process.resume().await?;
        }

        // Phase 2: Run resize2fs inside the guest via SSH (online ext4 resize).
        // Cloud-hypervisor (with config interrupt patch) notifies the guest of block
        // device size changes automatically via virtio config interrupt.
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let vm_host = vm.network.vm_addr.to_string();
        let ssh_private_key = PrivateKey::from(vm.config.ssh_keypair.clone());
        let expected_bytes = new_fs_size_mib as u64 * 1024 * 1024;

        // Check current size in guest
        let check_cmd = "blockdev --getsize64 /dev/vda";
        match exec_ssh(&ssh_private_key, &vm_host, check_cmd).await {
            Ok(()) => {
                tracing::info!(vm_id = %vm_id, "Checked guest block device size after rescan attempts");
            }
            Err(e) => {
                tracing::warn!(vm_id = %vm_id, error = %e, "Failed to check guest block device size");
            }
        }

        // Try resize2fs with polling
        let resize_cmd = format!(
            "timeout 60 bash -c 'attempt=0; while true; do current=$(blockdev --getsize64 /dev/vda 2>/dev/null); if [ -n \"$current\" ] && [ \"$current\" -ge {} ]; then break; fi; attempt=$((attempt+1)); if [ $attempt -gt 120 ]; then echo \"Timeout waiting for size\"; exit 124; fi; sleep 0.5; done; resize2fs -f /dev/vda'",
            expected_bytes
        );

        match exec_ssh(&ssh_private_key, &vm_host, &resize_cmd).await {
            Ok(()) => {
                tracing::info!(vm_id = %vm_id, "Successfully ran resize2fs in guest");
            }
            Err(e) => {
                // Check if it's a timeout (guest never saw new size) vs resize2fs error
                let error_str = e.to_string();
                if error_str.contains("exit 124")
                    || error_str.contains("timed out")
                    || error_str.contains("Timeout")
                {
                    tracing::warn!(
                        vm_id = %vm_id,
                        error = %e,
                        "Guest did not see new block device size (cloud-hypervisor limitation). \
                         Block device resized on host, user can manually run resize2fs"
                    );
                    // Don't return error - block device was resized, just guest didn't see it
                } else {
                    tracing::warn!(vm_id = %vm_id, error = %e, "resize2fs failed in guest");
                    // Don't return error - block device was resized
                }
            }
        }

        Ok(())
    }

    /// Commits a VM's state. If the VM is not paused, it will be paused, and automatically resumed when the commit is created. `wait_boot` specifies, if VM is booting, whether to wait for VM boot or return an error immediately.
    pub async fn commit_vm(
        &self,
        vm_id: &Uuid,
        commit_id: Uuid,
        keep_paused: bool,
        wait_boot: bool,
    ) -> Result<(), VmManagerError> {
        // If the VM is booting, either wait or error immediately
        if let VmBootSubscribeResult::Booting(mut receiver) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            match wait_boot {
                false => {
                    return Err(VmLifecycleError::StillBooting {
                        vm_id: vm_id.to_string(),
                    }
                    .into());
                }
                true => receiver.recv().await??,
            };
        };

        let vm = self.rehydrate_vm(vm_id).await?;
        if !vm.process.is_paused().await? {
            vm.process.pause().await?;
        }

        // Get the CPU architecture of the VM's host machine
        let host_architecture = get_host_cpu_architecture().await?;

        // Ensure there is enough space to create the commit
        let volume_id = vm.volume.id();
        let process_pid = vm.process.pid().await?;
        let space_required_mib = self.volume_manager.calculate_commit_size_mib(&volume_id)
            + self
                .process_manager
                .calculate_commit_size_mib(process_pid)
                .await?;

        self.commit_store.ensure_space(space_required_mib).await?;

        // Commit the volume and process state
        let (volume_result, process_result) = tokio::join!(
            self.volume_manager.commit_volume(&volume_id, &commit_id),
            self.process_manager.commit_process(process_pid, &commit_id),
        );
        let ((volume_to_upload, volume_metadata), (process_to_upload, process_metadata)) =
            (volume_result?, process_result?);

        // If keep_paused is false, then automatically resume the VM.
        if !keep_paused {
            vm.process.resume().await?;
        }

        // Collect commit files
        let to_upload = vec![volume_to_upload, process_to_upload].concat();

        // Upload the created files to the VmCommitStore
        let remote_files = self
            .commit_store
            .upload_commit_files(&commit_id, &to_upload)
            .await?;

        // Create the finished VmCommitMetadata struct
        let vm_commit_metadata = VmCommitMetadata {
            commit_id,
            host_architecture,
            process_metadata,
            volume_metadata,
            vm_config: vm.config.try_into()?,
            remote_files,
        };

        // Write the commit metadata to the manager's store
        self.remote_store
            .chelsea
            .commit
            .insert(&vm_commit_metadata.into())
            .await?;

        Ok(())
    }

    /// If `wait_boot` is true, waits for the specified VM to be booted. If false, immediately return an error if the VM is still booting.
    pub async fn wait_vm_booted(
        &self,
        vm_id: &Uuid,
        wait_boot: bool,
    ) -> Result<(), VmManagerError> {
        if let VmBootSubscribeResult::Booting(mut receiver) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            match wait_boot {
                false => {
                    return Err(VmLifecycleError::StillBooting {
                        vm_id: vm_id.to_string(),
                    }
                    .into());
                }
                true => receiver.recv().await??,
            };
        };

        Ok(())
    }

    /// Deletes the VM - and its associated resources - with the specified ID.
    pub async fn delete_vm(&self, vm_id: &Uuid) -> Result<(), VmManagerError> {
        let vm = self
            .local_store
            .fetch_vm_record(vm_id)
            .await?
            .ok_or_else(|| {
                anyhow!(VmLookupError::Vm {
                    vm_id: vm_id.to_string()
                })
            })?;

        // Attempt to kill the process first (network and volume resources may still be in use by it)
        let process_result = self.process_manager.kill(vm.vm_process_pid).await;

        let (
            network_result,
            volume_result,
            local_store_result,
            remote_store_delete_vm_result,
            remote_store_complete_segment_result,
        ) = tokio::join!(
            self.network_manager.on_vm_killed(&vm.vm_network_host_addr),
            self.volume_manager.on_vm_killed(&vm.vm_volume_id),
            // Delete VM record from local store
            self.local_store.delete_vm_record(vm_id),
            // Delete VM record from remote store
            self.remote_store.chelsea.vm.delete_by_id(vm_id),
            // Update VM usage segment with stop metadata in remote store
            {
                let vm_id = vm_id.clone();
                async move {
                    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
                        Ok(duration) => duration.as_secs() as i64,
                        Err(e) => return Err(anyhow::anyhow!(e)),
                    };
                    match self
                        .remote_store
                        .chelsea
                        .vm_usage_segment
                        .complete_latest_segment(&vm_id, now, now, None)
                        .await
                    {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(anyhow!(format!(
                            "No open VM usage segment found for {}",
                            vm_id
                        ))),
                        Err(e) => Err(anyhow!(e)),
                    }
                }
            }
        );

        // If there were any errors killing the VM, return them.
        let errors = [
            process_result,
            network_result,
            volume_result,
            local_store_result.map_err(|err| anyhow!(err)),
            match remote_store_delete_vm_result {
                Ok(_) => Ok(()),
                Err(err) => Err(anyhow!(err)),
            },
            match remote_store_complete_segment_result {
                Ok(_) => Ok(()),
                Err(err) => Err(anyhow!(err)),
            },
        ]
        .into_iter()
        .filter_map(|x| x.err())
        .collect::<Vec<_>>();
        if errors.len() > 0 {
            return Err(VmManagerError::Other(anyhow!(format!(
                "One or more errors while killing VM {}: {}",
                vm_id,
                join_errors(&errors, "; ")
            ))));
        }

        // Clean up the per-VM lock entry now that the VM is gone
        match self.vm_locks.lock() {
            Ok(mut map) => {
                map.remove(vm_id);
            }
            Err(e) => {
                error!(%vm_id, "Failed to clean up per-VM lock entry: vm_locks mutex poisoned: {e}");
            }
        }

        Ok(())
    }

    /// Reboots the VM by killing the process and restarting it, attaching it to the same resources it previously had.
    async fn reboot_vm(&self, vm_id: &Uuid) -> Result<(), VmManagerError> {
        // Kill the old process, skipping cleanup
        let old_vm = self.rehydrate_vm(vm_id).await?;
        if let Err(error) = self.process_manager.kill(old_vm.process.pid().await?).await {
            debug!(%error, "Error while killing VM process for reboot; attempting to continue");
        }

        // Inform the readiness service that the VM is booting.
        self.ready_service.insert_booting_vm(vm_id.clone()).await;

        // Defer informing the readiness service that VM boot has been aborted.
        let mut defer = DeferAsync::new();
        defer.defer({
            let ready_service = self.ready_service.clone();
            let vm_id = vm_id.clone();
            async move {
                if let Err(error) = ready_service
                    .remove_booting_vm(&vm_id, Err(VmBootError::Aborted))
                    .await
                {
                    warn!(%error, "Error while removing cleaned up VM from ReadyService");
                }
            }
        });

        // Recreate the process config based on hypervisor type
        let new_process_config = match old_vm.config.hypervisor_type {
            HypervisorType::Firecracker => {
                VmProcessConfig::Firecracker(FirecrackerProcessConfig::with_defaults(
                    vm_id.clone(),
                    &old_vm.volume,
                    &old_vm.network,
                    &old_vm.config,
                    &self.ready_service.chelsea_notify_boot_url_template,
                ))
            }
            HypervisorType::CloudHypervisor => VmProcessConfig::CloudHypervisor(
                CloudHypervisorProcessConfig::with_defaults(
                    vm_id.clone(),
                    &old_vm.volume,
                    &old_vm.network,
                    &old_vm.config,
                    &self.ready_service.chelsea_notify_boot_url_template,
                )
                .await,
            ),
        };

        // Ensure TAP device exists (CH deletes it on exit)
        old_vm.network.ensure_tap().await?;

        // Spawn a new process
        let new_process = self
            .process_manager
            .spawn_new(
                old_vm.id.clone(),
                &old_vm.config,
                &new_process_config,
                &old_vm.network.netns_name,
            )
            .await?;
        let new_process_pid = new_process.pid().await?;

        // Spawn vsock readiness monitor — races with HTTP notify-ready.
        tokio::spawn(wait_for_vsock_agent_ready(
            vm_id.clone(),
            old_vm.config.hypervisor_type,
            self.ready_service.clone(),
        ));

        // Defer cleaning up the process
        defer.defer({
            let process_manager = self.process_manager.clone();
            async move {
                if let Err(error) = process_manager.kill(new_process_pid).await {
                    warn!(%error, "Error while cleaning up newly-spawned VM process");
                }
            }
        });

        // Update the PID associated with the VM
        self.local_store
            .update_vm_process_pid(&old_vm.id, new_process_pid)
            .await?;

        // Spawn a task to await booting, cleaning up (via defer.cleanup()) if booting fails
        let _boot_task = tokio::spawn({
            let ready_service = self.ready_service.clone();
            let vm_id = vm_id.clone();

            async move {
                match ready_service.wait_vm_boot(&vm_id).await {
                    Ok(()) => {
                        defer.commit();
                        Ok(())
                    }
                    Err(error) => {
                        error!(%error, %vm_id, "VM failed to boot");
                        defer.cleanup().await;
                        notify_orchestrator_boot_failed(&vm_id).await;
                        Err(error)
                    }
                }
            }
        });

        Ok(())
    }

    /// Attempts to reboot the VM, and if that fails, deletes the VM.
    pub async fn reboot_or_delete_vm(&self, vm_id: &Uuid) -> Result<(), VmManagerError> {
        if let Err(error) = self.reboot_vm(vm_id).await {
            warn!(%error, %vm_id, "Failed to reboot VM; deleting now");
            self.delete_vm(vm_id).await?;
        }

        Ok(())
    }

    /// Get the current state of the specified VM.
    async fn get_vm_state(&self, vm_id: &Uuid) -> Result<VmState, VmManagerError> {
        if self
            .remote_store
            .chelsea
            .sleep_snapshot
            .fetch_latest_by_vm_id(vm_id)
            .await
            .is_ok()
        {
            return Ok(VmState::Sleeping);
        }

        if let VmBootSubscribeResult::Booting(_) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            return Ok(VmState::Booting);
        }

        let vm = self.rehydrate_vm(vm_id).await?;
        match vm.process.is_paused().await {
            Ok(true) => Ok(VmState::Paused),
            Ok(false) => Ok(VmState::Running),
            Err(e) => {
                // Process is likely dead (socket doesn't exist, etc.)
                tracing::warn!(
                    vm_id = %vm_id,
                    error = %e,
                    "Failed to query VM state, marking as dead"
                );
                Ok(VmState::Dead)
            }
        }
    }

    /// Get a summary of all VMs
    pub async fn list_all_vms(&self) -> Result<Vec<VmSummary>, VmManagerError> {
        let mut summaries = Vec::new();
        for id in self.local_store.list_all_vm_ids().await? {
            summaries.push(self.get_vm_summary(&id).await?);
        }
        Ok(summaries)
    }

    /// Get a summary of a specific VM
    pub async fn get_vm_summary(&self, vm_id: &Uuid) -> Result<VmSummary, VmManagerError> {
        let state = self.get_vm_state(vm_id).await?;
        Ok(VmSummary {
            vm_id: vm_id.to_string(),
            state,
        })
    }

    /// Return the total number of VMs that the VmManager allows to be created
    pub fn get_vm_count_max(&self) -> u64 {
        self.network_manager.get_vm_network_count() as u64
    }

    /// Return the number of VMs currently known to the VmManager
    pub async fn get_vm_count_current(&self) -> Result<u64, VmManagerError> {
        Ok(self.local_store.count_vms().await?)
    }

    /// Ensure that a new VM with the given sizes would not exceed hard maxima or current VM reservation limits. Note
    /// that these are theoretical limits and do not check actual host state; see technical docs for more information
    /// on allocation.
    pub async fn check_vm_reservation(
        &self,
        vcpu_count: u32,
        mem_size_mib: u32,
        volume_size_mib: u32,
    ) -> Result<(), VmManagerError> {
        // Check VM count
        let vm_count_current = self.get_vm_count_current().await?;
        let vm_count_max = self.get_vm_count_max();

        if vm_count_current >= vm_count_max {
            return Err(VmAllocationError::VmCountExceeded {
                current: vm_count_current,
                max: vm_count_max,
            }
            .into());
        }

        // Check RAM and vCPU reservation
        let vm_reservation = self.local_store.get_vm_resource_reservation().await?;

        // Hard maxima
        if vcpu_count > vm_reservation.vcpu_count.max {
            return Err(VmAllocationError::HardMaximumViolation {
                ty: VmAllocationType::Vcpu,
                requested: vcpu_count,
                max: vm_reservation.vcpu_count.max,
            }
            .into());
        }
        if mem_size_mib > vm_reservation.memory_mib.max {
            return Err(VmAllocationError::HardMaximumViolation {
                ty: VmAllocationType::Memory,
                requested: mem_size_mib,
                max: vm_reservation.memory_mib.max,
            }
            .into());
        }
        if volume_size_mib > vm_reservation.volume_mib.max {
            return Err(VmAllocationError::HardMaximumViolation {
                ty: VmAllocationType::Volume,
                requested: volume_size_mib,
                max: vm_reservation.volume_mib.max,
            }
            .into());
        }

        // Current reservation
        if vcpu_count > vm_reservation.vcpu_count.available() {
            return Err(VmAllocationError::InsufficientResources {
                ty: VmAllocationType::Vcpu,
                requested: vcpu_count,
                available: vm_reservation.vcpu_count.available(),
            }
            .into());
        }
        if mem_size_mib > vm_reservation.memory_mib.available() {
            return Err(VmAllocationError::InsufficientResources {
                ty: VmAllocationType::Memory,
                requested: mem_size_mib,
                available: vm_reservation.memory_mib.available(),
            }
            .into());
        }
        if volume_size_mib > vm_reservation.volume_mib.available() {
            return Err(VmAllocationError::InsufficientResources {
                ty: VmAllocationType::Volume,
                requested: volume_size_mib,
                available: vm_reservation.volume_mib.available(),
            }
            .into());
        }

        Ok(())
    }

    pub async fn get_vm_ssh_key_and_port(
        &self,
        vm_id: &Uuid,
    ) -> Result<(String, u16), VmManagerError> {
        // Refuse the request if the VM is paused
        let vm = self.rehydrate_vm(&vm_id).await?;
        if vm.process.is_paused().await? {
            return Err(VmManagerError::VmLifecycle(VmLifecycleError::IsPaused {
                vm_id: vm_id.to_string(),
            }));
        }

        let vm_record = self
            .local_store
            .fetch_vm_record(vm_id)
            .await?
            .ok_or_else(|| {
                anyhow!(VmLookupError::Vm {
                    vm_id: vm_id.to_string()
                })
            })?;

        let ssh_private_key = vm_record.ssh_private_key;
        let vm_network_host_addr = vm_record.vm_network_host_addr;
        let ssh_port = self
            .network_manager
            .store
            .fetch_vm_network(&vm_network_host_addr)
            .await?
            .ok_or_else(|| {
                anyhow!(VmLookupError::Network {
                    vm_network_host_addr
                })
            })?
            .ssh_port;

        Ok((ssh_private_key, ssh_port))
    }

    /// Get the network information associated with a particular VM
    pub async fn get_vm_network_info(
        &self,
        vm_id: &Uuid,
    ) -> Result<VmNetworkRecord, VmManagerError> {
        let vm = self.rehydrate_vm(vm_id).await?;
        Ok(VmNetworkRecord::from(&vm.network))
    }

    /// Look up the VM's network namespace and WireGuard interface name.
    pub async fn get_vm_wireguard_target(
        &self,
        vm_id: &Uuid,
    ) -> Result<(String, String), VmManagerError> {
        let vm_record = self
            .local_store
            .fetch_vm_record(vm_id)
            .await?
            .ok_or_else(|| {
                anyhow!(VmLookupError::Vm {
                    vm_id: vm_id.to_string()
                })
            })?;

        let network = self
            .network_manager
            .store
            .fetch_vm_network(&vm_record.vm_network_host_addr)
            .await?
            .ok_or_else(|| {
                anyhow!(VmLookupError::Network {
                    vm_network_host_addr: vm_record.vm_network_host_addr
                })
            })?;

        let wg = network
            .wg
            .as_ref()
            .ok_or_else(|| anyhow!("WireGuard config missing for VM {}", vm_id))?;

        Ok((network.netns_name.clone(), wg.interface_name.clone()))
    }

    /// Callback invoked when a VM sends an event to the host (legacy HTTP
    /// `notify-ready` path). Both the vsock monitor and the HTTP callback race
    /// — whichever fires first wins; the other is harmlessly ignored.
    pub async fn on_vm_event(&self, vm_id: &Uuid, event: VmEvent) {
        match event {
            VmEvent::Ready => {
                complete_vm_boot(&self.ready_service, vm_id, "http-notify").await;
            }
        }
    }

    // ── Exec operations (vsock agent) ────────────────────────────────────

    /// Execute a command in the VM and return the collected result.
    pub async fn exec_vm_command(
        &self,
        vm_id: &Uuid,
        request: agent_protocol::ExecRequest,
        wait_boot: bool,
    ) -> Result<agent_protocol::ExecResult, VmManagerError> {
        self.ensure_vm_ready(vm_id, wait_boot).await?;
        let client = VsockClient::new(vsock_socket_path(vm_id, self.hypervisor_type));
        let result = client.exec_with_options(request).await?;
        Ok(result)
    }

    /// Write a file into the VM via the vsock agent.
    pub async fn write_file(
        &self,
        vm_id: &Uuid,
        path: &str,
        content: &[u8],
        mode: u32,
        create_dirs: bool,
        wait_boot: bool,
    ) -> Result<(), VmManagerError> {
        self.ensure_vm_ready(vm_id, wait_boot).await?;
        let client = VsockClient::new(vsock_socket_path(vm_id, self.hypervisor_type));
        client.write_file(path, content, mode, create_dirs).await?;
        Ok(())
    }

    /// Read a file from the VM via the vsock agent.
    pub async fn read_file(
        &self,
        vm_id: &Uuid,
        path: &str,
        wait_boot: bool,
    ) -> Result<Vec<u8>, VmManagerError> {
        self.ensure_vm_ready(vm_id, wait_boot).await?;
        let client = VsockClient::new(vsock_socket_path(vm_id, self.hypervisor_type));
        let content = client.read_file(path).await?;
        Ok(content)
    }

    /// Start a streaming exec session in the VM.
    pub async fn exec_vm_stream(
        &self,
        vm_id: &Uuid,
        request: agent_protocol::ExecRequest,
        wait_boot: bool,
    ) -> Result<crate::vsock::ExecStreamConnection, VmManagerError> {
        self.ensure_vm_ready(vm_id, wait_boot).await?;
        let client = VsockClient::new(vsock_socket_path(vm_id, self.hypervisor_type));
        let conn = client.exec_stream(request).await?;
        Ok(conn)
    }

    /// Reattach to an existing streaming exec session.
    pub async fn exec_vm_stream_attach(
        &self,
        vm_id: &Uuid,
        exec_id: Uuid,
        cursor: Option<u64>,
        from_latest: bool,
        wait_boot: bool,
    ) -> Result<crate::vsock::ExecStreamConnection, VmManagerError> {
        self.ensure_vm_ready(vm_id, wait_boot).await?;
        let client = VsockClient::new(vsock_socket_path(vm_id, self.hypervisor_type));
        let conn = client
            .exec_stream_attach(exec_id, cursor, from_latest)
            .await?;
        Ok(conn)
    }

    /// Tail the exec log from the VM agent.
    pub async fn tail_exec_log(
        &self,
        vm_id: &Uuid,
        request: agent_protocol::TailExecLogRequest,
        wait_boot: bool,
    ) -> Result<agent_protocol::ExecLogChunkResponse, VmManagerError> {
        self.ensure_vm_ready(vm_id, wait_boot).await?;
        let client = VsockClient::new(vsock_socket_path(vm_id, self.hypervisor_type));
        let result = client.tail_exec_log(request).await?;
        Ok(result)
    }

    /// Update the agent binary inside a VM.
    ///
    /// Downloads the binary from `url`, verifies it against `expected_sha256`,
    /// and atomically replaces the running agent. If `restart` is true the
    /// agent restarts itself — the caller should reconnect afterward.
    pub async fn update_vm_agent(
        &self,
        vm_id: &Uuid,
        url: &str,
        expected_sha256: &str,
        restart: bool,
        wait_boot: bool,
    ) -> Result<(), VmManagerError> {
        self.ensure_vm_ready(vm_id, wait_boot).await?;
        let client = VsockClient::new(vsock_socket_path(vm_id, self.hypervisor_type));
        client.update_agent(url, expected_sha256, restart).await?;
        Ok(())
    }

    /// Wait for the VM to finish booting if requested, or return an error if
    /// it is still booting and `wait_boot` is false.
    async fn ensure_vm_ready(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), VmManagerError> {
        if let VmBootSubscribeResult::Booting(mut receiver) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            if wait_boot {
                receiver.recv().await??;
            } else {
                return Err(VmLifecycleError::StillBooting {
                    vm_id: vm_id.to_string(),
                }
                .into());
            }
        }
        Ok(())
    }

    /// Creates a temporary snapshot of the specified VM, then kills the associated process. This temporary snapshot is used via wake_vm
    pub async fn sleep_vm(&self, vm_id: &Uuid, wait_boot: bool) -> Result<(), VmManagerError> {
        // If the VM is booting, either wait or error immediately
        if let VmBootSubscribeResult::Booting(mut receiver) =
            self.ready_service.subscribe_to_vm_boot_event(vm_id).await?
        {
            match wait_boot {
                false => {
                    return Err(VmLifecycleError::StillBooting {
                        vm_id: vm_id.to_string(),
                    }
                    .into());
                }
                true => receiver.recv().await??,
            };
        };

        // If the VM isn't paused, pause it
        let vm = self.rehydrate_vm(vm_id).await?;
        if !vm.process.is_paused().await? {
            vm.process.pause().await?;
        }

        // Get the CPU architecture of the VM's host machine
        let host_architecture = get_host_cpu_architecture().await?;

        // Ensure there is enough space to create the sleep snapshot
        let volume_id = vm.volume.id();
        let process_pid = vm.process.pid().await?;
        let space_required_mib = self
            .volume_manager
            .calculate_sleep_snapshot_size_mib(&volume_id)
            + self
                .process_manager
                .calculate_sleep_snapshot_size_mib(process_pid)
                .await?;

        self.sleep_snapshot_store
            .ensure_space(space_required_mib)
            .await?;

        // Generate a unique ID for this snapshot
        let snapshot_id = Uuid::new_v4();

        // Create a sleep snapshot of the volume and process state
        let (volume_result, process_result) = tokio::join!(
            self.volume_manager.sleep_snapshot_volume(&volume_id),
            self.process_manager
                .sleep_snapshot_process(process_pid, &snapshot_id),
        );
        let ((volume_to_upload, volume_metadata), (process_to_upload, process_metadata)) =
            (volume_result?, process_result?);

        // Collect snapshot files
        let to_upload = vec![volume_to_upload, process_to_upload].concat();

        // Upload the created files to the VmSnapshotStore
        let remote_files = self
            .sleep_snapshot_store
            .upload_sleep_snapshot_files(&snapshot_id, &to_upload)
            .await?;

        // Write the snapshot metadata to the manager's store
        self.remote_store
            .chelsea
            .sleep_snapshot
            .insert(
                &snapshot_id,
                &vm.id,
                &host_architecture,
                &vm.config.kernel_name,
                &vm.config.base_image,
                vm.config.vcpu_count,
                vm.config.mem_size_mib,
                vm.config.fs_size_mib,
                &vm.config.ssh_keypair.public.to_string(),
                &PrivateKey::from(vm.config.ssh_keypair)
                    .to_openssh(ssh_key::LineEnding::LF)?
                    .to_string(),
                &process_metadata.into(),
                &volume_metadata.into(),
                &remote_files,
            )
            .await?;

        // Attempt to kill the process first (network and volume resources may still be in use by it)
        let process_result = self.process_manager.on_vm_sleep(process_pid).await;

        let (
            network_result,
            volume_result,
            local_store_result,
            remote_store_complete_segment_result,
        ) = tokio::join!(
            self.network_manager.on_vm_sleep(&vm.network.host_addr),
            self.volume_manager.on_vm_sleep(&volume_id),
            // Delete VM record from local store
            self.local_store.delete_vm_record(vm_id),
            // Update VM usage segment with stop metadata in remote store
            {
                let vm_id = vm_id.clone();
                async move {
                    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
                        Ok(duration) => duration.as_secs() as i64,
                        Err(e) => return Err(anyhow::anyhow!(e)),
                    };
                    match self
                        .remote_store
                        .chelsea
                        .vm_usage_segment
                        .complete_latest_segment(&vm_id, now, now, None)
                        .await
                    {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(anyhow!(format!(
                            "No open VM usage segment found for {}",
                            vm_id
                        ))),
                        Err(e) => Err(anyhow!(e)),
                    }
                }
            }
        );

        // If there were any errors sleeping the VM, return them.
        let errors = [
            process_result,
            network_result,
            volume_result,
            local_store_result.map_err(|err| anyhow!(err)),
            match remote_store_complete_segment_result {
                Ok(_) => Ok(()),
                Err(err) => Err(anyhow!(err)),
            },
        ]
        .into_iter()
        .filter_map(|x| x.err())
        .collect::<Vec<_>>();
        if errors.len() > 0 {
            return Err(VmManagerError::Other(anyhow!(format!(
                "One or more errors while sleeping VM {}: {}",
                vm_id,
                join_errors(&errors, "; ")
            ))));
        }

        Ok(())
    }

    /// Starts a VM from a temporary snapshot created by a sleep_vm invocation
    pub async fn wake_vm(&self, vm_id: &Uuid, wg: VmWireGuardConfig) -> Result<(), VmManagerError> {
        if self.get_vm_state(vm_id).await? != VmState::Sleeping {
            return Err(VmManagerError::VmLifecycle(
                VmLifecycleError::IsNotSleeping {
                    vm_id: vm_id.clone(),
                },
            ));
        }

        // Fetch the snapshot metadata from the persistent store
        let sleep_snapshot_metadata = self
            .remote_store
            .chelsea
            .sleep_snapshot
            .fetch_latest_by_vm_id(vm_id)
            .await?;

        // Check that the requested VM size would not exceed configured maxima or current resource availability
        self.check_vm_reservation(
            sleep_snapshot_metadata.vcpu_count,
            sleep_snapshot_metadata.mem_size_mib,
            sleep_snapshot_metadata.fs_size_mib,
        )
        .await?;

        // Download all associated commit files to the commit data directory
        self.sleep_snapshot_store
            .download_sleep_snapshot_files(&sleep_snapshot_metadata.remote_files)
            .await?;

        // Create the network for the VM
        let network = self.network_manager.reserve_network().await?;

        // Defer deleting the network
        let mut defer = DeferAsync::new();
        defer.defer({
            let network_manager = self.network_manager.clone();
            let host_addr = network.host_addr.clone();
            async move {
                if let Err(error) = network_manager.on_vm_killed(&host_addr).await {
                    error!(
                        %error,
                        ?host_addr,
                        "Error while cleaning up VM network via manager"
                    );
                    if let Err(unreserve_error) =
                        network_manager.release_reserved_network(&host_addr).await
                    {
                        error!(
                            %unreserve_error,
                            ?host_addr,
                            "Fallback network release also failed while cleaning up VM network"
                        );
                    }
                }
            }
        });

        // Create the new volume for the VM from the snapshot metadata.
        let volume = self
            .volume_manager
            .create_volume_from_sleep_snapshot_record(
                &sleep_snapshot_metadata.volume_sleep_snapshot,
            )
            .await?;

        // Defer deleting the volume.
        defer.defer({
            let volume_manager = self.volume_manager.clone();
            let volume_clone = volume.clone();
            let volume_id = volume_clone.id();

            async move {
                if let Err(error) = volume_manager.on_vm_killed(&volume_id).await {
                    error!(
                        %error,
                        vm_volume_id = %volume_id,
                        "Error while cleaning up VM volume via manager"
                    );
                    if let Err(delete_error) = volume_clone.delete().await {
                        error!(
                            %delete_error,
                            vm_volume_id = %volume_id,
                            "Fallback volume delete also failed while cleaning up VM volume"
                        );
                    }
                }
            }
        });

        // Create a VmConfig from the sleep snapshot metadata
        let RecordSleepSnapshot {
            kernel_name,
            base_image,
            vcpu_count,
            mem_size_mib,
            fs_size_mib,
            ssh_private_key,
            ..
        } = sleep_snapshot_metadata.clone();

        let ssh_keypair = PrivateKey::from_openssh(ssh_private_key)?
            .key_data()
            .ed25519()
            .ok_or(VmManagerError::Other(anyhow!(
                "Failed to extract ED25519 private key from snapshot record"
            )))?
            .clone();

        // Determine hypervisor type from sleep snapshot metadata
        let hypervisor_type = match &sleep_snapshot_metadata.process_sleep_snapshot {
            vers_pg::schema::chelsea::tables::sleep_snapshot::RecordProcessSleepSnapshot::Firecracker(_) => HypervisorType::Firecracker,
            vers_pg::schema::chelsea::tables::sleep_snapshot::RecordProcessSleepSnapshot::CloudHypervisor(_) => HypervisorType::CloudHypervisor,
        };

        let vm_config = VmConfig {
            kernel_name,
            base_image,
            vcpu_count,
            mem_size_mib,
            fs_size_mib,
            ssh_keypair,
            hypervisor_type,
        };

        // Ensure TAP device exists (CH deletes it on exit, need to recreate)
        network.ensure_tap().await?;

        // Spawn process
        let process = self
            .process_manager
            .spawn_from_sleep_snapshot(
                vm_id,
                &sleep_snapshot_metadata.id,
                &vm_config,
                &sleep_snapshot_metadata.process_sleep_snapshot,
                &network,
                volume.path().as_path(),
            )
            .await?;

        // Defer cleaning up VM process
        let process_pid = process.pid().await?;
        let process_manager = self.process_manager.clone();
        let process_clone = process.clone();
        defer.defer(async move {
            if let Err(error) = process_manager.kill(process_pid).await {
                error!(
                    %error,
                    vm_process_pid = process_pid,
                    "Error while cleaning up VM process via manager"
                );
                if let Err(kill_error) = process_clone.kill().await {
                    error!(
                        %kill_error,
                        vm_process_pid = process_pid,
                        "Fallback process kill also failed while cleaning up VM process"
                    );
                }
            }
        });

        // Inform the network manager that a VM has had Wireguard config attached to it.
        self.network_manager
            .on_vm_created(&network.host_addr, wg)
            .await?;

        // Defer cleaning up network manager state (on_vm_killed)
        defer.defer({
            let network_manager = self.network_manager.clone();
            let host_addr = network.host_addr;
            async move {
                if let Err(error) = network_manager.on_vm_killed(&host_addr).await {
                    error!(
                        %error,
                        vm_network_host_addr = %host_addr,
                        "Error while cleaning up network manager state via on_vm_killed"
                    );
                }
            }
        });

        // Create VM struct
        let vm = Vm::new(vm_id.clone(), vm_config, process, network, volume);

        // Insert VM record into local store
        let record = vm
            .as_record()
            .await
            .context("Interpreting new Vm as record")?;
        self.local_store
            .insert_vm_record(record)
            .await
            .context("Inserting new Vm into store")?;

        // Defer deleting VM record from local store
        defer.defer({
            let store = self.local_store.clone();
            let vm_id = vm_id.clone();
            async move {
                if let Err(error) = store.delete_vm_record(&vm_id).await {
                    error!(
                        %error,
                        %vm_id,
                        "Error while cleaning up VM record from store"
                    );
                }
            }
        });

        // Delete used snapshot from remote store
        self.remote_store
            .chelsea
            .sleep_snapshot
            .soft_delete_by_id(&sleep_snapshot_metadata.id)
            .await?;

        // Delete snapshot files from snapshot store
        self.sleep_snapshot_store
            .delete_sleep_snapshot_files(&sleep_snapshot_metadata.remote_files)
            .await?;

        // Insert VM usage start event into remote store
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        self.remote_store
            .chelsea
            .vm_usage_segment
            .insert_start(&RecordVmUsageSegmentStart {
                vm_id: vm_id.clone(),
                start_timestamp: now,
                start_created_at: now,
                vcpu_count: vm.config.vcpu_count,
                ram_mib: vm.config.mem_size_mib,
                disk_gib: None,
                start_code: None,
            })
            .await?;

        defer.commit();
        Ok(())
    }
}

/// Notify the orchestrator that a VM failed to boot so it can mark the
/// DB record as deleted. This is a best-effort fire-and-forget call;
/// if it fails, the reconciliation loop will catch it eventually.
async fn notify_orchestrator_boot_failed(vm_id: &Uuid) {
    use vers_config::VersConfig;

    let orch = VersConfig::orchestrator();
    let url = format!(
        "http://[{}]:{}/api/v1/internal/vm/{}/boot-failed",
        orch.wg_private_ip, orch.port, vm_id
    );

    match reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {}", orch.admin_api_key))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(%vm_id, "Notified orchestrator of boot failure");
        }
        Ok(resp) => {
            tracing::warn!(
                %vm_id,
                status = %resp.status(),
                "Orchestrator returned non-success for boot-failed notification"
            );
        }
        Err(err) => {
            tracing::warn!(
                %vm_id,
                %err,
                "Failed to notify orchestrator of boot failure (reconciliation will catch it)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ready_service::store::VmReadyServiceStore;
    use async_trait::async_trait;

    /// A mock store for testing the ready service without a real database.
    struct MockReadyStore;

    #[async_trait]
    impl VmReadyServiceStore for MockReadyStore {
        async fn vm_exists(
            &self,
            _vm_id: &Uuid,
        ) -> Result<bool, crate::ready_service::error::VmReadyServiceStoreError> {
            Ok(true)
        }
    }

    fn test_ready_service() -> VmReadyService {
        VmReadyService::new(
            Arc::new(MockReadyStore),
            "http://test:80/api/vm/{vm_id}/notify".to_string(),
        )
    }

    #[test]
    fn test_vsock_socket_path_firecracker_contains_vm_id() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let path = vsock_socket_path(&vm_id, HypervisorType::Firecracker);

        assert!(
            path.to_string_lossy().contains(&vm_id.to_string()),
            "vsock socket path should contain VM ID, got: {}",
            path.display()
        );
        assert!(
            path.to_string_lossy().ends_with("run/vsock.sock"),
            "vsock socket path should end with run/vsock.sock, got: {}",
            path.display()
        );
    }

    #[test]
    fn test_vsock_socket_path_cloud_hypervisor_contains_vm_id() {
        let vm_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let path = vsock_socket_path(&vm_id, HypervisorType::CloudHypervisor);

        assert!(
            path.to_string_lossy().contains(&vm_id.to_string()),
            "vsock socket path should contain VM ID, got: {}",
            path.display()
        );
        assert!(
            path.to_string_lossy().ends_with("run/vsock.sock"),
            "vsock socket path should end with run/vsock.sock, got: {}",
            path.display()
        );
    }

    #[test]
    fn test_vsock_socket_path_different_vms_get_different_paths() {
        let vm_id1 = Uuid::new_v4();
        let vm_id2 = Uuid::new_v4();
        assert_ne!(
            vsock_socket_path(&vm_id1, HypervisorType::Firecracker),
            vsock_socket_path(&vm_id2, HypervisorType::Firecracker)
        );
    }

    #[tokio::test]
    async fn test_complete_vm_boot_already_completed_is_not_error() {
        // complete_vm_boot should handle NotFound (already completed by other
        // path) gracefully — it should not panic or return an error.
        let ready_service = test_ready_service();
        let vm_id = Uuid::new_v4();
        // Don't insert into booting map — simulates the case where the other
        // path already completed boot.
        complete_vm_boot(&ready_service, &vm_id, "test").await;
        // If we get here without panicking, the test passes.
    }

    #[tokio::test]
    async fn test_complete_vm_boot_first_caller_wins() {
        let ready_service = test_ready_service();
        let vm_id = Uuid::new_v4();
        ready_service.insert_booting_vm(vm_id).await;

        // First call should succeed (marks VM as booted)
        complete_vm_boot(&ready_service, &vm_id, "vsock").await;

        // Second call should be harmless (already completed)
        complete_vm_boot(&ready_service, &vm_id, "http-notify").await;
        // No panic = success
    }

    #[tokio::test]
    async fn test_complete_vm_boot_concurrent_callers() {
        // Simulate the actual race: two tasks calling complete_vm_boot
        // concurrently on the same VM. Exactly one should "win".
        let ready_service = Arc::new(test_ready_service());
        let vm_id = Uuid::new_v4();
        ready_service.insert_booting_vm(vm_id).await;

        let rs1 = ready_service.clone();
        let rs2 = ready_service.clone();

        let (r1, r2) = tokio::join!(
            tokio::spawn(async move { complete_vm_boot(&rs1, &vm_id, "vsock").await }),
            tokio::spawn(async move { complete_vm_boot(&rs2, &vm_id, "http").await }),
        );
        // Neither should panic
        r1.expect("vsock task panicked");
        r2.expect("http task panicked");
    }

    #[tokio::test]
    async fn test_complete_vm_boot_subscriber_receives_result() {
        // A subscriber waiting on boot should receive Ok(()) regardless of
        // which path completes first.
        let ready_service = test_ready_service();
        let vm_id = Uuid::new_v4();
        ready_service.insert_booting_vm(vm_id).await;

        // Subscribe before completing
        if let VmBootSubscribeResult::Booting(mut receiver) = ready_service
            .subscribe_to_vm_boot_event(&vm_id)
            .await
            .unwrap()
        {
            // Complete boot via vsock path
            complete_vm_boot(&ready_service, &vm_id, "vsock").await;

            // Subscriber should receive Ok(())
            let result = receiver.recv().await.unwrap();
            assert!(
                result.is_ok(),
                "subscriber should receive Ok, got: {:?}",
                result
            );
        } else {
            panic!("expected Booting state after insert_booting_vm");
        }
    }

    #[tokio::test]
    async fn test_ensure_vm_ready_not_booting() {
        // A VM that's not in the booting map is considered ready.
        // ensure_vm_ready should return Ok immediately.
        let ready_service = Arc::new(test_ready_service());
        let vm_id = Uuid::new_v4();

        // Build a minimal VmManager-like check using the ready service directly
        let result = ready_service.subscribe_to_vm_boot_event(&vm_id).await;
        match result {
            Ok(VmBootSubscribeResult::NotBooting) => {
                // This is what ensure_vm_ready sees for a ready VM — Ok
            }
            _ => panic!("expected NotBooting"),
        }
    }

    #[tokio::test]
    async fn test_ensure_vm_ready_still_booting_no_wait() {
        // A VM that IS booting + wait_boot=false should yield StillBooting.
        let ready_service = Arc::new(test_ready_service());
        let vm_id = Uuid::new_v4();
        ready_service.insert_booting_vm(vm_id).await;

        let result = ready_service.subscribe_to_vm_boot_event(&vm_id).await;
        match result {
            Ok(VmBootSubscribeResult::Booting(_)) => {
                // ensure_vm_ready with wait_boot=false returns StillBooting here
            }
            _ => panic!("expected Booting"),
        }
    }

    #[tokio::test]
    async fn test_ensure_vm_ready_booting_then_completes() {
        // VM is booting, wait_boot=true, boot completes → Ok
        let ready_service = Arc::new(test_ready_service());
        let vm_id = Uuid::new_v4();
        ready_service.insert_booting_vm(vm_id).await;

        let rs = ready_service.clone();
        let handle = tokio::spawn(async move {
            if let VmBootSubscribeResult::Booting(mut receiver) =
                rs.subscribe_to_vm_boot_event(&vm_id).await.unwrap()
            {
                receiver.recv().await.unwrap().unwrap();
            }
        });

        // Simulate boot completing after a short delay
        tokio::time::sleep(Duration::from_millis(10)).await;
        complete_vm_boot(&ready_service, &vm_id, "vsock").await;

        // The waiting task should complete without error
        handle.await.expect("boot waiter panicked");
    }

    #[tokio::test]
    async fn test_complete_vm_boot_after_timeout_is_harmless() {
        // Scenario: boot times out (someone else calls remove_booting_vm
        // with Err(Timeout)), then vsock monitor calls complete_vm_boot.
        // The second call must not panic — it gets NotFound and logs it.
        let ready_service = test_ready_service();
        let vm_id = Uuid::new_v4();
        ready_service.insert_booting_vm(vm_id).await;

        // Simulate timeout: remove with Err
        ready_service
            .remove_booting_vm(&vm_id, Err(VmBootError::Timeout))
            .await
            .expect("remove_booting_vm should succeed");

        // Now vsock monitor arrives late — should be harmless
        complete_vm_boot(&ready_service, &vm_id, "vsock").await;
        // No panic = success; the VM was already cleaned up by timeout
    }

    #[tokio::test]
    async fn test_complete_vm_boot_timeout_subscriber_sees_error() {
        // Verify that when boot times out, the subscriber actually gets the
        // Timeout error (not Ok), even if complete_vm_boot is called later.
        let ready_service = test_ready_service();
        let vm_id = Uuid::new_v4();
        ready_service.insert_booting_vm(vm_id).await;

        // Subscribe
        let mut receiver = match ready_service
            .subscribe_to_vm_boot_event(&vm_id)
            .await
            .expect("subscribe should succeed")
        {
            VmBootSubscribeResult::Booting(rx) => rx,
            _ => panic!("expected Booting"),
        };

        // Simulate timeout
        ready_service
            .remove_booting_vm(&vm_id, Err(VmBootError::Timeout))
            .await
            .expect("remove should succeed");

        // Subscriber should see Timeout error
        let result = receiver.recv().await.expect("should receive");
        assert!(result.is_err(), "subscriber should see Timeout error");
        match result.unwrap_err() {
            VmBootError::Timeout => {} // expected
            other => panic!("expected Timeout, got: {:?}", other),
        }

        // Late vsock call is harmless
        complete_vm_boot(&ready_service, &vm_id, "vsock").await;
    }
}

#[cfg(test)]
mod env_var_tests {
    use super::*;

    #[test]
    fn render_env_file_empty() {
        let vars = HashMap::new();
        let content = render_env_file(&vars);
        assert!(content.is_empty());
    }

    #[test]
    fn render_env_file_sorted() {
        let mut vars = HashMap::new();
        vars.insert("ZZZ".to_string(), "last".to_string());
        vars.insert("AAA".to_string(), "first".to_string());
        vars.insert("MMM".to_string(), "middle".to_string());

        let content = render_env_file(&vars);
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "AAA=first");
        assert_eq!(lines[1], "MMM=middle");
        assert_eq!(lines[2], "ZZZ=last");
    }

    #[test]
    fn render_env_file_no_export_prefix() {
        let mut vars = HashMap::new();
        vars.insert("DB_URL".to_string(), "postgres://localhost/db".to_string());

        let content = render_env_file(&vars);
        assert_eq!(content, "DB_URL=postgres://localhost/db\n");
        assert!(!content.contains("export"));
    }

    #[test]
    fn render_env_file_values_unquoted() {
        let mut vars = HashMap::new();
        vars.insert("MSG".to_string(), "hello world".to_string());

        let content = render_env_file(&vars);
        assert_eq!(content, "MSG=hello world\n");
    }

    #[test]
    fn render_env_file_special_chars_literal() {
        let mut vars = HashMap::new();
        vars.insert("A".to_string(), "$(rm -rf /)".to_string());
        vars.insert("B".to_string(), "`cat /etc/passwd`".to_string());
        vars.insert("C".to_string(), "${HOME}".to_string());

        let content = render_env_file(&vars);
        assert!(content.contains("A=$(rm -rf /)\n"));
        assert!(content.contains("B=`cat /etc/passwd`\n"));
        assert!(content.contains("C=${HOME}\n"));
    }

    #[test]
    fn render_env_file_empty_value() {
        let mut vars = HashMap::new();
        vars.insert("EMPTY".to_string(), "".to_string());

        let content = render_env_file(&vars);
        assert_eq!(content, "EMPTY=\n");
    }

    #[test]
    fn render_env_file_value_with_equals() {
        let mut vars = HashMap::new();
        vars.insert("DSN".to_string(), "host=localhost port=5432".to_string());

        let content = render_env_file(&vars);
        assert_eq!(content, "DSN=host=localhost port=5432\n");
    }

    #[test]
    fn render_env_file_single_quotes_in_value() {
        let mut vars = HashMap::new();
        vars.insert("MSG".to_string(), "it's fine".to_string());

        let content = render_env_file(&vars);
        assert_eq!(content, "MSG=it's fine\n");
    }
}
