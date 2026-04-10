use std::path::PathBuf;

use crate::volume::volume::VmVolume;

use async_trait::async_trait;
pub use ceph::ThinVolume as CephVmVolume;
use uuid::Uuid;

#[async_trait]
impl VmVolume for CephVmVolume {
    fn path(&self) -> PathBuf {
        self.path().to_path_buf()
    }

    fn id(&self) -> Uuid {
        self.id.clone()
    }

    fn image_name(&self) -> String {
        self.image_name.clone()
    }

    async fn delete(&self) -> anyhow::Result<()> {
        self.delete().await?;
        Ok(())
    }

    async fn grow(&self, vm_volume_size_mib: u32) -> anyhow::Result<()> {
        self.grow(vm_volume_size_mib).await?;
        Ok(())
    }

    async fn grow_device_only(&self, vm_volume_size_mib: u32) -> anyhow::Result<()> {
        self.grow_device_only(vm_volume_size_mib).await?;
        Ok(())
    }
}
