use thiserror::Error;
use uuid::Uuid;

use crate::db::{DB, DBError, VMsRepository, VmEntity};

/// Lists the VMs created with said api key.
#[derive(Debug, Clone)]
pub struct ListOwnerVMs {
    pub owner_api_key_id: Uuid,
}
impl ListOwnerVMs {
    pub fn for_owner(owner_api_key_id: Uuid) -> Self {
        Self { owner_api_key_id }
    }
}

#[derive(Debug, Error)]
pub enum ListOwnerVMsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
}

impl ListOwnerVMs {
    pub async fn call(self, db: &DB) -> Result<Vec<VmEntity>, ListOwnerVMsError> {
        let vms = db.vms().list_by_api_key(self.owner_api_key_id).await?;
        Ok(vms)
    }
}
