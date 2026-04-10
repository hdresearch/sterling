use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use ceph::{RbdClient, RbdClientError, RbdSnapName, ThinVolume};
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use util::defer::DeferAsync;
use uuid::Uuid;
use vers_config::VersConfig;
use vers_pg::schema::chelsea::tables::sleep_snapshot::RecordVolumeSleepSnapshot;

use crate::{
    volume::VmVolume,
    volume_manager::{
        VmVolumeCommitMetadata, VmVolumeManager,
        ceph::{
            CephVmVolumeCommitMetadata, CephVmVolumeSleepSnapshotMetadata,
            store::CephVmVolumeRecord,
        },
        error::CreateVmVolumeFromImageError,
        sleep_snapshot::VmVolumeSleepSnapshotMetadata,
    },
};

use super::{CephVmVolumeManagerStore, DefaultVolumePool};

pub struct CephVmVolumeManager {
    /// The manager's local store
    store: Arc<dyn CephVmVolumeManagerStore>,
    rbd_client: RbdClient,
    /// Pre-warmed volume pool for default image (if enabled)
    pool: Option<Arc<DefaultVolumePool>>,
    /// Cache of base image sizes (image_name → size_mib).
    /// Populated at startup and on first access; avoids repeated `rbd info` subprocess calls.
    base_image_size_cache: RwLock<HashMap<String, u32>>,
}

impl CephVmVolumeManager {
    pub async fn new(local_store: Arc<dyn CephVmVolumeManagerStore>) -> anyhow::Result<Self> {
        let config = VersConfig::chelsea();
        let rbd_client = RbdClient::new(
            "chelsea".to_string(),
            "rbd".to_string(),
            Duration::from_secs(config.ceph_client_timeout_secs as u64),
        )?;
        // Ensure we are able to connect to the cluster.
        info!("Ensuring connection to Ceph cluster...");
        rbd_client
            .ceph_status()
            .await
            .context("Failed to ensure connection to Ceph cluster on VolumeManager init")?;
        info!("Successfully connected to Ceph cluster.");

        // Initialize the pre-warmed volume pool if enabled
        let pool = if config.default_volume_pool_enabled {
            Some(Arc::new(DefaultVolumePool::new(
                config.vm_default_image_name.clone(),
                config.default_volume_pool_size,
            )))
        } else {
            None
        };

        // Pre-populate the base image size cache for the default image
        let mut base_image_size_cache = HashMap::new();
        match rbd_client.image_info(&config.vm_default_image_name).await {
            Ok(info) => {
                let size = info.size_mib();
                info!(
                    image = %config.vm_default_image_name,
                    size_mib = size,
                    "Cached default base image size"
                );
                base_image_size_cache.insert(config.vm_default_image_name.clone(), size);
            }
            Err(e) => {
                debug!(
                    image = %config.vm_default_image_name,
                    %e,
                    "Could not cache default base image size (will fetch on demand)"
                );
            }
        }

        Ok(Self {
            store: local_store,
            rbd_client,
            pool,
            base_image_size_cache: RwLock::new(base_image_size_cache),
        })
    }

    /// Start the background replenishment task for the volume pool
    pub async fn start_pool(&self) {
        if let Some(pool) = &self.pool {
            pool.start_background_replenishment().await;
        }
    }

    /// Shutdown the volume pool gracefully
    pub async fn shutdown_pool(&self) {
        if let Some(pool) = &self.pool {
            pool.shutdown().await;
        }
    }

    /// Standard volume creation path (clone + map from snapshot)
    async fn create_volume_standard(
        &self,
        image_name: &str,
        id: Uuid,
    ) -> Result<Arc<ThinVolume>, CreateVmVolumeFromImageError> {
        let snap_name = RbdSnapName {
            image_name: image_name.to_string(),
            snap_name: VersConfig::chelsea().ceph_base_image_snap_name.clone(),
        };
        let volume = Arc::new(ThinVolume::new_mapped_from_snap(id, &snap_name).await?);
        Ok(volume)
    }

    pub async fn rehydrate_ceph_vm_volume(
        &self,
        vm_volume_id: &Uuid,
    ) -> anyhow::Result<ThinVolume> {
        let vm_volume_record = self
            .store
            .fetch_ceph_vm_volume_record(vm_volume_id)
            .await?
            .ok_or(anyhow!(
                "Failed to find Ceph VmVolume with id '{vm_volume_id}'."
            ))?;

        Ok(ThinVolume::from_existing(
            vm_volume_record.id,
            vm_volume_record.image_name,
            PathBuf::from(vm_volume_record.device_path),
        ))
    }

    /// Unmap the specified volume and delete it from the store
    async fn delete_volume(&self, vm_volume_id: &Uuid) -> anyhow::Result<()> {
        let volume = self.rehydrate_ceph_vm_volume(vm_volume_id).await?;

        let volume_path = volume.path();
        let vm_volume_id_c = *vm_volume_id;

        let (device_unmap_result, local_delete_result) = tokio::join!(
            // Unmap RBD from host
            self.rbd_client.device_unmap(volume_path),
            // Delete record from local store
            self.store.delete_ceph_vm_volume_record(&vm_volume_id_c),
        );

        let errors = [
            device_unmap_result.map_err(|e| anyhow!(e)),
            local_delete_result.map_err(|e| anyhow!(e)),
        ]
        .into_iter()
        .filter_map(|res| res.err())
        .collect::<Vec<_>>();

        if !errors.is_empty() {
            Err(anyhow!(format!(
                "One or more errors while deleting Ceph volume {}: {}",
                vm_volume_id,
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            )))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl VmVolumeManager for CephVmVolumeManager {
    async fn get_base_image_size_mib(
        &self,
        image_name: &str,
    ) -> Result<u32, CreateVmVolumeFromImageError> {
        // Fast path: return cached size if available
        if let Some(&size) = self.base_image_size_cache.read().await.get(image_name) {
            return Ok(size);
        }

        // Slow path: fetch from Ceph and cache the result
        let size = self
            .rbd_client
            .image_info(image_name)
            .await
            .map_err(|e| match e {
                RbdClientError::NotFound(_) => {
                    CreateVmVolumeFromImageError::ImageNotFound(image_name.to_string())
                }
                other => CreateVmVolumeFromImageError::Other(other.to_string()),
            })?
            .size_mib();

        self.base_image_size_cache
            .write()
            .await
            .insert(image_name.to_string(), size);

        Ok(size)
    }

    /// Looks for a snapshot called {image_name}@{VersConfig::ceph_base_image_snap_name}, clones it, and creates a new volume from it with the requested size.
    async fn create_volume_from_base_image(
        &self,
        image_name: String,
        vm_volume_size_mib: u32,
    ) -> Result<Arc<dyn VmVolume>, CreateVmVolumeFromImageError> {
        let config = VersConfig::chelsea();
        let id = Uuid::new_v4();

        // Determine whether we can use the pre-warmed pool and log the decision
        let pool_skip_reason = if image_name != config.vm_default_image_name {
            Some(format!("non-default image '{}'", image_name))
        } else if self.pool.is_none() {
            Some("pool disabled".to_string())
        } else {
            if vm_volume_size_mib < config.vm_default_fs_size_mib {
                Some(format!(
                    "requested size ({} MiB) < pre-warmed size ({} MiB), cannot shrink",
                    vm_volume_size_mib, config.vm_default_fs_size_mib
                ))
            } else {
                None // Can use pool
            }
        };

        let can_use_pool = pool_skip_reason.is_none();

        // Try to use the pre-warmed pool if appropriate.
        // `known_size_mib` tracks the volume's current size when already known,
        // avoiding a costly `rbd info` subprocess call.
        let (volume, known_size_mib): (Arc<ThinVolume>, Option<u32>) = if can_use_pool {
            if let Some(pool) = &self.pool {
                let requested_size_mib = vm_volume_size_mib;
                if let Some((prewarmed, prewarmed_size)) =
                    pool.try_acquire(id, requested_size_mib).await
                {
                    debug!(
                        volume_id = %id,
                        requested_size_mib = vm_volume_size_mib,
                        prewarmed_size_mib = prewarmed_size,
                        "Using pre-warmed volume from pool"
                    );
                    (Arc::new(prewarmed), Some(prewarmed_size))
                } else {
                    debug!(
                        volume_id = %id,
                        requested_size_mib = vm_volume_size_mib,
                        "Pool empty, falling back to standard volume creation"
                    );
                    (self.create_volume_standard(&image_name, id).await?, None)
                }
            } else {
                // Shouldn't reach here due to can_use_pool check, but handle gracefully
                (self.create_volume_standard(&image_name, id).await?, None)
            }
        } else {
            // Use standard path for non-default images or when size requirements don't match
            if image_name != config.vm_default_image_name
                && !self.rbd_client.image_exists(&image_name).await?
            {
                return Err(CreateVmVolumeFromImageError::ImageNotFound(image_name));
            }

            debug!(
                volume_id = %id,
                image_name = %image_name,
                requested_size_mib = vm_volume_size_mib,
                prewarmed_size_mib = config.vm_default_fs_size_mib,
                reason = pool_skip_reason.as_deref().unwrap_or("unknown"),
                "Skipping pre-warmed pool, using standard volume creation"
            );
            (self.create_volume_standard(&image_name, id).await?, None)
        };

        // Defer deleting the volume
        let mut defer = DeferAsync::new();
        defer.defer({
            let volume = volume.clone();
            async move {
                if let Err(error) = volume.delete().await {
                    error!(%error, "Error while cleaning up Ceph ThinVolume");
                }
            }
        });

        // Grow the volume to the requested size (skip if already correct size).
        // Use the known size from the pool when available to avoid an `rbd info` call.
        let current_size_mib = match known_size_mib {
            Some(size) => size,
            None => volume.get_size_mib().await?,
        };
        let requested_size_mib = vm_volume_size_mib;
        if current_size_mib != requested_size_mib {
            debug!(
                volume_id = %id,
                current_size_mib,
                requested_size_mib,
                "Resizing volume"
            );
            volume
                .grow(requested_size_mib)
                .await
                .map_err(|e| CreateVmVolumeFromImageError::Other(e.to_string()))?;
        } else {
            debug!(
                volume_id = %id,
                size_mib = vm_volume_size_mib,
                "Skipping resize, volume already correct size"
            );
        }

        // Store the record in the local store
        let record = CephVmVolumeRecord::from_ceph_vm_volume(volume.clone()).await;
        self.store
            .insert_ceph_vm_volume_record(record)
            .await
            .map_err(|e| CreateVmVolumeFromImageError::Other(e.to_string()))?;

        defer.commit();
        Ok(volume)
    }

    async fn create_volume_from_volume(
        &self,
        volume_id: &Uuid,
    ) -> anyhow::Result<Arc<dyn VmVolume>> {
        let parent = self.rehydrate_ceph_vm_volume(volume_id).await?;
        let child_id = Uuid::new_v4();

        // Create the child volume
        let child = Arc::new(
            parent
                .create_child_mapped(child_id, &parent.create_snap().await?)
                .await?,
        );

        // Defer deleting the child volume
        let mut defer = DeferAsync::new();
        defer.defer({
            let child = child.clone();
            async move {
                if let Err(error) = child.delete().await {
                    error!(%error, "Error while cleaning up Ceph ThinVolume");
                }
            }
        });

        // Insert record into manager's store
        let record = CephVmVolumeRecord::from_ceph_vm_volume(child.clone()).await;
        self.store
            .insert_ceph_vm_volume_record(record)
            .await
            .map_err(|e| anyhow!(CreateVmVolumeFromImageError::Other(e.to_string())))?;

        defer.commit();
        Ok(child as Arc<dyn VmVolume>)
    }

    async fn rehydrate_vm_volume(&self, volume_id: &Uuid) -> anyhow::Result<Arc<dyn VmVolume>> {
        self.rehydrate_ceph_vm_volume(volume_id)
            .await
            .map(|x| Arc::new(x) as Arc<dyn VmVolume>)
    }

    /// Create a snapshot of the provided volume, and returns commit metadata linking the two
    async fn commit_volume(
        &self,
        volume_id: &Uuid,
        _commit_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, VmVolumeCommitMetadata)> {
        let volume = self.rehydrate_ceph_vm_volume(volume_id).await?;

        // Create Ceph snap
        let snap_name = volume.create_snap().await?;

        let volume_commit_metadata =
            VmVolumeCommitMetadata::Ceph(CephVmVolumeCommitMetadata { snap_name });

        // No files created; returning empty vec
        Ok((Vec::new(), volume_commit_metadata))
    }

    async fn sleep_snapshot_volume(
        &self,
        volume_id: &Uuid,
    ) -> anyhow::Result<(Vec<String>, VmVolumeSleepSnapshotMetadata)> {
        // Sleep snapshots simply reuse the same image
        let volume = self.rehydrate_ceph_vm_volume(volume_id).await?;

        let volume_sleep_snapshot_metadata =
            VmVolumeSleepSnapshotMetadata::Ceph(CephVmVolumeSleepSnapshotMetadata {
                image_name: volume.image_name,
            });
        Ok((Vec::new(), volume_sleep_snapshot_metadata))
    }

    async fn create_volume_from_commit_metadata(
        &self,
        volume_commit_metadata: &VmVolumeCommitMetadata,
    ) -> anyhow::Result<Arc<dyn VmVolume>> {
        // CephVmVolumeManagers can only handle VmVolumeCommitMetadata::Ceph variants; currently, this should be the only variant.
        let VmVolumeCommitMetadata::Ceph(volume_commit_metadata) = volume_commit_metadata;
        let snap_name = &volume_commit_metadata.snap_name;

        // Create the volume
        let volume_id = Uuid::new_v4();
        let volume = Arc::new(ThinVolume::new_mapped_from_snap(volume_id, snap_name).await?);

        // Defer deleting the volume
        let mut defer = DeferAsync::new();
        defer.defer({
            let volume = volume.clone();
            async move {
                if let Err(error) = volume.delete().await {
                    error!(%error, "Error while cleaning up Ceph ThinVolume");
                }
            }
        });

        let record = CephVmVolumeRecord::from_ceph_vm_volume(volume.clone()).await;
        self.store
            .insert_ceph_vm_volume_record(record)
            .await
            .map_err(|e| anyhow!(CreateVmVolumeFromImageError::Other(e.to_string())))?;

        defer.commit();
        Ok(volume)
    }

    async fn create_volume_from_sleep_snapshot_record(
        &self,
        volume_sleep_snapshot_metadata: &RecordVolumeSleepSnapshot,
    ) -> anyhow::Result<Arc<dyn VmVolume>> {
        // CephVmVolumeManagers can only handle VmVolumeCommitMetadata::Ceph variants; currently, this should be the only variant.
        let RecordVolumeSleepSnapshot::Ceph(volume_sleep_snapshot_metadata) =
            volume_sleep_snapshot_metadata;
        let image_name = &volume_sleep_snapshot_metadata.image_name;

        // Create the volume
        let volume_id = Uuid::new_v4();
        let volume = Arc::new(ThinVolume::new_mapped_from_image(volume_id, image_name).await?);

        // Defer deleting the volume
        let mut defer = DeferAsync::new();
        defer.defer({
            let volume = volume.clone();
            async move {
                if let Err(error) = volume.delete().await {
                    error!(%error, "Error while cleaning up Ceph ThinVolume");
                }
            }
        });

        // Store the new volume record
        let record = CephVmVolumeRecord::from_ceph_vm_volume(volume.clone()).await;
        self.store
            .insert_ceph_vm_volume_record(record)
            .await
            .map_err(|e| anyhow!(CreateVmVolumeFromImageError::Other(e.to_string())))?;

        defer.commit();
        Ok(volume)
    }

    async fn resize_volume(
        &self,
        vm_volume_id: &Uuid,
        vm_volume_size_mib: u32,
    ) -> anyhow::Result<()> {
        let volume = self.rehydrate_vm_volume(vm_volume_id).await?;

        volume.grow(vm_volume_size_mib).await?;

        Ok(())
    }

    async fn resize_volume_device_only(
        &self,
        vm_volume_id: &Uuid,
        vm_volume_size_mib: u32,
    ) -> anyhow::Result<()> {
        let volume = self.rehydrate_vm_volume(vm_volume_id).await?;

        volume.grow_device_only(vm_volume_size_mib).await?;

        Ok(())
    }

    async fn on_vm_killed(&self, vm_volume_id: &Uuid) -> anyhow::Result<()> {
        // TODO: Also recursively delete ancestors whose refcounts have reached 0
        self.delete_volume(vm_volume_id).await
    }

    async fn on_vm_sleep(&self, vm_volume_id: &Uuid) -> anyhow::Result<()> {
        self.delete_volume(vm_volume_id).await
    }

    async fn on_vm_resumed(&self, _vm_volume_id: &Uuid) -> anyhow::Result<()> {
        // Currently, the manager doesn't care about this.
        Ok(())
    }

    fn calculate_commit_size_mib(&self, _vm_volume_id: &Uuid) -> u32 {
        // A ceph volume commit creates no files on disk
        0
    }

    fn calculate_sleep_snapshot_size_mib(&self, _vm_volume_id: &Uuid) -> u32 {
        // Ceph takes no action when sleeping besides unmapping the specified image
        0
    }
}

impl From<RbdClientError> for CreateVmVolumeFromImageError {
    fn from(value: RbdClientError) -> Self {
        Self::Other(value.to_string())
    }
}
