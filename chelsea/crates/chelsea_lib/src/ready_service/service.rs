use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{
    sync::{Mutex, broadcast},
    task::JoinHandle,
};
use tracing::error;
use uuid::Uuid;
use vers_config::VersConfig;

use crate::ready_service::{
    VmReadyServiceStore,
    error::{VmBootError, VmReadyServiceError},
};

/// The result from attempting to subscribe to a particular VM's boot event
pub enum VmBootSubscribeResult {
    /// The VM is not booting. Note that this doesn't imply success or failure to boot; only that the VM is not currently booting
    NotBooting,
    /// The VM is booting. This
    Booting(broadcast::Receiver<Result<(), VmBootError>>),
}

/// Represents a VM in the process of booting
struct BootingVm {
    /// Used to broadcast Ok(()) when the VM is successfully booted, and Err(VmBootError) otherwise.
    pub sender: broadcast::Sender<Result<(), VmBootError>>,
    /// The task to be canceled when an outside caller calls remove_booting_vm(). Spawned to ensure no records get stuck in the VmReadyService. Must be canceled.
    pub timeout_task_handle: JoinHandle<()>,
}

/// A service that manages VMs while they're in the bootup phase. This happens whenever a new root VM is created. Currently, the way
/// we track whether a VM is booted up or not is via a userspace systemd service injected into the VM (see `fetch_fs.sh`.)
pub struct VmReadyService {
    /// A list of VM IDs that are regarded as still booting. Includes a broadcast sender for boot event subscribers that sends Ok on successful boot, Err otherwise
    /// Note that this list is purely in-memory, since we rely on a running process anyway: https://github.com/hdresearch/chelsea/issues/422
    booting_vms: Arc<Mutex<HashMap<Uuid, BootingVm>>>,
    /// A pointer to the service's persistent store.
    store: Arc<dyn VmReadyServiceStore>,
    /// The chelsea_notify_boot_url_template variable expected by notify-ready.service. Must be passed to VMs as an environment variable.
    pub chelsea_notify_boot_url_template: String,
}

impl VmReadyService {
    /// Create a new VmReadyService
    pub fn new(
        store: Arc<dyn VmReadyServiceStore>,
        chelsea_notify_boot_url_template: String,
    ) -> Self {
        Self {
            booting_vms: Arc::new(Mutex::new(HashMap::new())),
            store,
            chelsea_notify_boot_url_template,
        }
    }

    /// Inform the service that a given VM ID should be regarded as booting. The ReadyService will spawn a task to time out the boot if remove_booting_vm is not called.
    pub async fn insert_booting_vm(&self, vm_id: Uuid) {
        let timeout_task_handle = tokio::spawn({
            let booting_vms = self.booting_vms.clone();
            async move {
                let timeout_duration =
                    Duration::from_secs(VersConfig::chelsea().vm_boot_timeout_secs);
                tokio::time::sleep(timeout_duration).await;

                // Task not canceled, attempt to remove booting VM.
                match booting_vms.lock().await.remove(&vm_id) {
                    None => error!(%vm_id, "Timeout task unable to find booting VM"),
                    Some(booting_vm) => {
                        // Send only returns an error when there are 0 receivers; this is completely ignorable.
                        let _ = booting_vm.sender.send(Err(VmBootError::Timeout));
                    }
                }
            }
        });

        let booting_vm = BootingVm {
            // Capacity 1; only one message will ever be sent on VM boot.
            sender: broadcast::Sender::new(1),
            timeout_task_handle,
        };

        self.booting_vms
            .lock()
            .await
            .insert(vm_id.clone(), booting_vm);
    }

    /// Inform the service that a given VM ID is finished booting, and whether or not the boot was successful.
    pub async fn remove_booting_vm(
        &self,
        vm_id: &Uuid,
        boot_result: Result<(), VmBootError>,
    ) -> Result<(), VmReadyServiceError> {
        match self.booting_vms.lock().await.remove(vm_id) {
            None => Err(VmReadyServiceError::NotFound(vm_id.to_string())),
            Some(booting_vm) => {
                // Send only returns an error when there are 0 receivers; this is completely ignorable.
                let _ = booting_vm.sender.send(boot_result);
                // Abort the timeout task; VM is being manually signaled here.
                booting_vm.timeout_task_handle.abort();
                Ok(())
            }
        }
    }

    /// Subscribe to the VM booting event. Timeouts are handled internally by BootingVm.timeout_task_handle; see
    /// VmReadyService.insert_booting_vm.
    pub async fn subscribe_to_vm_boot_event(
        &self,
        vm_id: &Uuid,
    ) -> Result<VmBootSubscribeResult, VmReadyServiceError> {
        if !self.store.vm_exists(vm_id).await? {
            Err(VmReadyServiceError::NotFound(vm_id.to_string()))
        } else if let Some(booting_vm) = self.booting_vms.lock().await.get(vm_id) {
            Ok(VmBootSubscribeResult::Booting(
                booting_vm.sender.subscribe(),
            ))
        } else {
            Ok(VmBootSubscribeResult::NotBooting)
        }
    }

    /// If the VM is booting, wait for it to complete, else return immediately. Timeouts are handled internally by
    /// BootingVm.timeout_task_handle; see VmReadyService.insert_booting_vm.
    pub async fn wait_vm_boot(&self, vm_id: &Uuid) -> Result<(), VmReadyServiceError> {
        match self.subscribe_to_vm_boot_event(vm_id).await? {
            VmBootSubscribeResult::NotBooting => Ok(()),
            VmBootSubscribeResult::Booting(mut receiver) => Ok(receiver.recv().await??),
        }
    }
}
