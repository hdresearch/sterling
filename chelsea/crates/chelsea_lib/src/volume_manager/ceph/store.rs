use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{store_error::StoreError, volume::ceph::CephVmVolume};

#[async_trait]
pub trait CephVmVolumeManagerStore: Send + Sync {
    async fn insert_ceph_vm_volume_record(
        &self,
        record: CephVmVolumeRecord,
    ) -> Result<(), StoreError>;
    async fn fetch_ceph_vm_volume_record(
        &self,
        vm_volume_id: &Uuid,
    ) -> Result<Option<CephVmVolumeRecord>, StoreError>;
    async fn delete_ceph_vm_volume_record(&self, vm_volume_id: &Uuid) -> Result<(), StoreError>;
}

pub struct CephVmVolumeRecord {
    pub id: Uuid,
    pub image_name: String,
    pub device_path: String,
}

impl CephVmVolumeRecord {
    pub async fn from_ceph_vm_volume(value: Arc<CephVmVolume>) -> Self {
        Self {
            id: value.id.clone(),
            image_name: value.image_name.clone(),
            device_path: value.device_path.to_string_lossy().to_string(),
        }
    }
}
