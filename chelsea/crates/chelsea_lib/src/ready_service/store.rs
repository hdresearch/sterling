use async_trait::async_trait;
use uuid::Uuid;

use crate::ready_service::error::VmReadyServiceStoreError;

/// A trait to be implemented by a store capable of serving required information for the VmReadyService
#[async_trait]
pub trait VmReadyServiceStore: Send + Sync {
    /// Whether or not a VM with the given ID exists
    async fn vm_exists(&self, vm_id: &Uuid) -> Result<bool, VmReadyServiceStoreError>;
}
