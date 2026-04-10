use super::{
    BackingFile, BackingFileCreateOptions, LoopDevice, PhysicalVolume, RootVolumeCreateOptions,
    ThinPool, ThinPoolCreateOptions, ThinVolume, VolumeGroup, VolumeGroupCreateOptions,
};
use crate::error::LvmError;
use serde::{Deserialize, Serialize};
use std::{result::Result::Ok, sync::Arc};
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredFsCreateOptions {
    pub id: String,
    /// When the LayeredFs creates its backing file, it will use this name (within the app's data directory). Default: chelsea-manager.img
    pub backing_file_name: String,
    /// The size of the filesystem, in megabytes.
    pub size_mib: u32,
    /// The volume group name. Default: chelsea
    pub volume_group_name: String,
    /// The thin pool name. Default: pool
    pub thin_pool_name: String,
    /// The base name for a volume spawned from the pool. Default: vm
    pub thin_pool_volume_name: String,
    /// The name of the root thin volume. Default: root
    pub root_volume_name: String,
    /// The size of the root volume.
    pub root_volume_size_mib: u32,
    /// Whether or not to format the root volume (with an ext4 filesystem)
    pub should_format_root_volume: bool,
}

impl Default for LayeredFsCreateOptions {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            backing_file_name: "chelsea-manager.img".to_string(),
            size_mib: 2 * 1024, // 2 GB
            volume_group_name: "chelsea".to_string(),
            thin_pool_name: "pool".to_string(),
            thin_pool_volume_name: "vm".to_string(),
            root_volume_name: "root".to_string(),
            root_volume_size_mib: 512,
            should_format_root_volume: true,
        }
    }
}

/// Represents the CreateOptions structs that are derived from a single LayeredFsCreateOptions, needed to create the individual
/// LVM resources needed by the LayeredFS.
#[derive(Serialize, Deserialize, Debug)]
pub struct LvmCreateOptions {
    id: String,
    backing_file_options: BackingFileCreateOptions,
    volume_group_options: VolumeGroupCreateOptions,
    thin_pool_options: ThinPoolCreateOptions,
    root_volume_options: RootVolumeCreateOptions,
}

impl From<LayeredFsCreateOptions> for LvmCreateOptions {
    fn from(value: LayeredFsCreateOptions) -> Self {
        let backing_file_options = BackingFileCreateOptions {
            filename: value.backing_file_name,
            size_mib: value.size_mib,
        };

        let volume_group_options = VolumeGroupCreateOptions {
            name: value.volume_group_name,
        };

        let thin_pool_options = ThinPoolCreateOptions {
            name: value.thin_pool_name,
        };

        let root_volume_size_mib = value.root_volume_size_mib;
        let root_volume_options = RootVolumeCreateOptions {
            name: value.root_volume_name,
            size_mib: root_volume_size_mib,
            should_format: value.should_format_root_volume,
        };

        Self {
            id: value.id,
            backing_file_options,
            volume_group_options,
            thin_pool_options,
            root_volume_options,
        }
    }
}

#[derive(Debug)]
pub struct LayeredFs {
    pub id: String,
    pub cluster_id: String,
    /// The root volume in the FS
    pub root_volume: Arc<ThinVolume>,
    /// The thin pool in the FS
    pub thin_pool: Arc<ThinPool>,
    /// The size of the filesystem, in megabytes
    pub size_cluster_mib: u32,
    /// The size of VMs within the FS, in megabytes
    pub size_vm_mib: u32,
}

impl LayeredFs {
    pub async fn new(
        options: LayeredFsCreateOptions,
        cluster_id: String,
    ) -> Result<Self, LvmError> {
        let size_cluster_mib = options.size_mib;
        let size_vm_mib = options.root_volume_size_mib;

        let options = LvmCreateOptions::from(options);

        let backing_file = BackingFile::new(options.backing_file_options).await?;
        let device = LoopDevice::new(backing_file).await?;
        let physical_volume = PhysicalVolume::new(device).await?;
        let volume_group =
            VolumeGroup::new(vec![physical_volume], options.volume_group_options).await?;
        let thin_pool = Arc::new(ThinPool::new(volume_group, options.thin_pool_options).await?);
        let root_volume =
            ThinVolume::new_root(thin_pool.clone(), options.root_volume_options).await?;

        Ok(Self {
            id: options.id,
            cluster_id,
            root_volume,
            thin_pool,
            size_cluster_mib,
            size_vm_mib,
        })
    }

    pub fn from_existing(
        id: String,
        cluster_id: String,
        root_volume: Arc<ThinVolume>,
        thin_pool: Arc<ThinPool>,
        size_cluster_mib: u32,
        size_vm_mib: u32,
    ) -> Result<Self, LvmError> {
        Ok(Self {
            id,
            cluster_id,
            root_volume,
            thin_pool,
            size_cluster_mib,
            size_vm_mib,
        })
    }

    pub async fn delete(&self) -> Result<(), LvmError> {
        debug!("Deleting thin pool");
        self.thin_pool.delete().await?;
        debug!("Successfully deleted layered filesystem");
        Ok(())
    }
}
