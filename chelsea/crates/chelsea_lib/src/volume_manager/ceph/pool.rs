use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Instant};

use ceph::{RbdSnapName, ThinVolume, default_rbd_client};
use tokio::sync::{Mutex, OnceCell, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use vers_config::VersConfig;

/// A pre-warmed volume ready for immediate use
struct PrewarmedVolume {
    /// The RBD image name (a UUID string)
    image_name: String,
    /// The mapped device path (e.g., /dev/rbd0)
    device_path: PathBuf,
    /// The volume size in MiB, recorded at creation time
    size_mib: u32,
    /// When this volume was created
    created_at: Instant,
}

/// Pool of pre-warmed volumes for the default base image
pub struct DefaultVolumePool {
    /// The base image name (e.g., "default")
    base_image_name: String,
    /// Target number of volumes to keep ready
    target_size: usize,
    /// Available pre-warmed volumes
    available: Arc<Mutex<VecDeque<PrewarmedVolume>>>,
    /// Shutdown signal sender
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// Ensures background replenishment is only started once
    started: OnceCell<()>,
}

impl DefaultVolumePool {
    pub fn new(base_image_name: String, target_size: usize) -> Self {
        Self {
            base_image_name,
            target_size,
            available: Arc::new(Mutex::new(VecDeque::with_capacity(target_size))),
            shutdown_tx: Mutex::new(None),
            started: OnceCell::const_new(),
        }
    }

    /// Start background replenishment task
    pub async fn start_background_replenishment(&self) {
        self.started
            .get_or_init(|| async {
                let (tx, mut rx) = oneshot::channel();
                *self.shutdown_tx.lock().await = Some(tx);

                let available = self.available.clone();
                let base_image_name = self.base_image_name.clone();
                let target_size = self.target_size;

                tokio::spawn(async move {
                    info!(
                        base_image = %base_image_name,
                        target_size,
                        "Starting default volume pool replenishment task"
                    );

                    loop {
                        // Check if we should shutdown
                        match rx.try_recv() {
                            Ok(_) | Err(oneshot::error::TryRecvError::Closed) => {
                                info!("Volume pool replenishment task shutting down");
                                break;
                            }
                            Err(oneshot::error::TryRecvError::Empty) => {}
                        }

                        // Check current pool size
                        let current_size = available.lock().await.len();

                        if current_size < target_size {
                            debug!(
                                current_size,
                                target_size, "Pool below target, creating new volume"
                            );

                            match Self::create_prewarmed_volume(&base_image_name).await {
                                Ok(volume) => {
                                    let mut guard = available.lock().await;
                                    guard.push_back(volume);
                                    let pool_size = guard.len();
                                    drop(guard);
                                    info!(pool_size, "Added pre-warmed volume to pool");
                                }
                                Err(e) => {
                                    error!(%e, "Failed to create pre-warmed volume");
                                    // Wait before retrying on error
                                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                                }
                            }
                        } else {
                            // Pool is full, sleep before checking again
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    }
                });
            })
            .await;
    }

    /// Create a single pre-warmed volume, pre-sized to default VM size
    async fn create_prewarmed_volume(base_image_name: &str) -> anyhow::Result<PrewarmedVolume> {
        let config = VersConfig::chelsea();
        let snap_name = RbdSnapName {
            image_name: base_image_name.to_string(),
            snap_name: config.ceph_base_image_snap_name.clone(),
        };

        // This performs clone + map
        let temp_id = Uuid::new_v4();
        let volume = ThinVolume::new_mapped_from_snap(temp_id, &snap_name).await?;

        // Pre-size to default VM size (avoids resize during VM creation)
        let default_size = config.vm_default_fs_size_mib;
        volume.grow(default_size).await?;

        Ok(PrewarmedVolume {
            image_name: volume.image_name,
            device_path: volume.device_path,
            size_mib: default_size,
            created_at: Instant::now(),
        })
    }

    /// Try to acquire a pre-warmed volume. Returns None if pool is empty or if the requested
    /// size is smaller than the pre-warmed volume size (we cannot shrink volumes).
    /// Try to acquire a pre-warmed volume, returning it along with its known size.
    /// Returns `None` if the pool is empty or if the requested size is smaller
    /// than the pre-warmed volume (volumes cannot be shrunk).
    pub async fn try_acquire(
        &self,
        volume_id: Uuid,
        fs_size_mib: u32,
    ) -> Option<(ThinVolume, u32)> {
        let prewarmed = self.available.lock().await.pop_front()?;

        // Safety check: Don't use pre-warmed volume if requested size is smaller
        // (we can grow volumes but cannot shrink them)
        let prewarmed_size_mib = prewarmed.size_mib;
        if fs_size_mib < prewarmed_size_mib {
            warn!(
                volume_id = %volume_id,
                requested_size_mib = fs_size_mib,
                prewarmed_size_mib,
                "Cannot use pre-warmed volume: requested size is smaller (volumes cannot be shrunk). Returning volume to pool."
            );
            // Return the volume back to the pool since we can't use it
            self.available.lock().await.push_front(prewarmed);
            return None;
        }

        debug!(
            volume_id = %volume_id,
            image_name = %prewarmed.image_name,
            requested_size_mib = fs_size_mib,
            prewarmed_size_mib,
            age_ms = prewarmed.created_at.elapsed().as_millis(),
            "Acquired pre-warmed volume from pool"
        );

        // Create a ThinVolume with the actual VM's volume id
        let size = prewarmed.size_mib;
        Some((
            ThinVolume::from_existing(volume_id, prewarmed.image_name, prewarmed.device_path),
            size,
        ))
    }

    /// Get current pool size (for monitoring)
    pub async fn pool_size(&self) -> usize {
        self.available.lock().await.len()
    }

    /// Graceful shutdown - unmaps all pooled volumes
    pub async fn shutdown(&self) {
        info!("Shutting down default volume pool");

        // Signal the background task to stop
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }

        // Unmap all pre-warmed volumes
        let client = match default_rbd_client() {
            Ok(c) => c,
            Err(e) => {
                error!(%e, "Failed to get RBD client for pool shutdown");
                return;
            }
        };

        let mut available = self.available.lock().await;
        let count = available.len();

        while let Some(volume) = available.pop_front() {
            // First unmap the device
            if let Err(e) = client.device_unmap(&volume.device_path).await {
                warn!(
                    device_path = %volume.device_path.display(),
                    %e,
                    "Failed to unmap pre-warmed volume during shutdown"
                );
                // Continue to try deleting even if unmap fails
            }

            // Then delete the RBD image to prevent orphaned images
            if let Err(e) = client.image_remove(&volume.image_name).await {
                warn!(
                    image_name = %volume.image_name,
                    %e,
                    "Failed to delete pre-warmed RBD image during shutdown"
                );
            }
        }

        info!(count, "Cleaned up pre-warmed volumes during shutdown");
    }
}
