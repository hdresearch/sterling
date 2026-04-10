use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    network::VmNetwork,
    store_error::StoreError,
    vm_manager::{record::VmRecord, types::VmReservation},
};

/// An interface that allows the manager to store, and retrieve, information about VMs.
#[async_trait]
pub trait VmManagerStore: Send + Sync {
    /// Get the number of vCPUs and the RAM, in MiB, currently allocated to running VMs.
    async fn get_vm_vcpu_and_ram_usage(&self) -> Result<(u32, u32), StoreError>;
    /// Persist the given VmRecord to the store.
    async fn insert_vm_record(&self, vm: VmRecord) -> Result<(), StoreError>;
    /// Fetch a VmRecord by its id
    async fn fetch_vm_record(&self, id: &Uuid) -> Result<Option<VmRecord>, StoreError>;
    /// Delete the VmRecord with the given id
    async fn delete_vm_record(&self, id: &Uuid) -> Result<(), StoreError>;
    /// List all VM ids
    async fn list_all_vm_ids(&self) -> Result<Vec<Uuid>, StoreError>;
    /// List all VM ids with their associated pids
    async fn list_all_vms_with_pids(&self) -> Result<Vec<(Uuid, u32)>, StoreError>;
    /// Returns the number of VMs in the store
    async fn count_vms(&self) -> Result<u64, StoreError>;
    /// Fetch a VM together with its associated network record.
    async fn fetch_vm_with_network(
        &self,
        id: &Uuid,
    ) -> Result<Option<(VmRecord, Option<VmNetwork>)>, StoreError>;
    /// Update the vm_process_pid of the given VM
    async fn update_vm_process_pid(&self, id: &Uuid, pid: u32) -> Result<(), StoreError>;

    /// Update the fs_size_mib of the given VM
    async fn update_vm_fs_size_mib(&self, id: &Uuid, fs_size_mib: u32) -> Result<(), StoreError>;

    /// Returns the current VM resource allocation. This should reflect the sum of all currently-running VMs' reserved vCPU and memory count.
    async fn get_vm_resource_reservation(&self) -> Result<VmReservation, StoreError>;
}
