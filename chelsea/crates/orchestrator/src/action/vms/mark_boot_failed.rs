use thiserror::Error;
use uuid::Uuid;

use crate::db::{DB, DBError, VMsRepository};

/// Mark a VM as deleted after Chelsea reports a boot failure.
///
/// Called by Chelsea nodes via the internal API when a VM fails to boot
/// and Chelsea has cleaned it up locally. This ensures the orchestrator
/// DB stays in sync without waiting for the reconciliation loop.
#[derive(Debug, Clone)]
pub struct MarkVmBootFailed {
    pub vm_id: Uuid,
}

impl MarkVmBootFailed {
    pub fn new(vm_id: Uuid) -> Self {
        Self { vm_id }
    }
}

#[derive(Debug, Error)]
pub enum MarkVmBootFailedError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
}

impl MarkVmBootFailed {
    pub async fn call(self, db: &DB) -> Result<(), MarkVmBootFailedError> {
        tracing::info!(vm_id = %self.vm_id, "Marking VM as deleted due to boot failure reported by Chelsea");
        db.vms().mark_deleted(&self.vm_id).await?;
        Ok(())
    }
}
