use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use uuid::Uuid;
use vers_config::HypervisorType;

use crate::{process::VmProcess, store_error::StoreError};

/// Represents a store capable of persisting process-specific data, such as PID
#[async_trait]
pub trait VmProcessManagerStore: Send + Sync {
    async fn insert_vm_process_record(
        &self,
        vm_process: &VmProcessRecord,
    ) -> Result<(), StoreError>;
    async fn fetch_vm_process_record(
        &self,
        pid: u32,
    ) -> Result<Option<VmProcessRecord>, StoreError>;
    async fn delete_vm_process_record(&self, pid: u32) -> Result<(), StoreError>;
}

/// Represents a VM process in the store
pub struct VmProcessRecord {
    pub pid: u32,
    pub process_type: HypervisorType,
    pub vm_id: Uuid,
}

impl VmProcessRecord {
    pub async fn try_from_vm_process(vm_process: &Arc<VmProcess>) -> anyhow::Result<Self> {
        Ok(Self {
            pid: vm_process
                .pid()
                .await
                .context("VmProcessRecord from VmProcess")?,
            process_type: vm_process.process_type(),
            vm_id: vm_process.vm_id(),
        })
    }
}
