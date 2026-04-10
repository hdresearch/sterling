//! Compatibility layer that provides the old `ceph` crate's API backed by
//! native cephalopod calls. Drop-in replacement — same type names, same
//! method signatures, same error variants.
//!
//! Usage: replace `use ceph::{...}` with `use cephalopod::compat::{...}`

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use crate::CephalopodError;
use crate::client::Client;
use crate::snap_name::RbdSnapName;

// ---------------------------------------------------------------------------
// RbdClientError — mirrors the old error type
// ---------------------------------------------------------------------------

/// Drop-in replacement for `ceph::RbdClientError`.
#[derive(thiserror::Error, Debug, Clone)]
pub enum RbdClientError {
    #[error("Failed to execute rbd: {0}")]
    Exec(String),
    #[error("Rbd exited with status code {0}\nstdout:{1}\nstderr:{2}")]
    ExitCode(i32, String, String),
    #[error("Failed to find keyring at expected path: {0}")]
    KeyringNotFound(PathBuf),
    #[error("Resource not found: {0}")]
    NotFound(String),
    #[error("Rbd client error: {0}")]
    Other(String),
}

impl From<CephalopodError> for RbdClientError {
    fn from(e: CephalopodError) -> Self {
        match e {
            CephalopodError::NotFound(msg) => RbdClientError::NotFound(msg),
            CephalopodError::AlreadyExists(msg) => RbdClientError::ExitCode(17, String::new(), msg),
            CephalopodError::Ceph {
                errno,
                message,
                context,
            } => RbdClientError::ExitCode(errno, context, message),
            CephalopodError::NulByte(e) => RbdClientError::Exec(e.to_string()),
            CephalopodError::Device(msg) => RbdClientError::Other(msg),
        }
    }
}

// ---------------------------------------------------------------------------
// RbdImageInfo — mirrors the old type with fields consumers actually use
// ---------------------------------------------------------------------------

/// Drop-in replacement for `ceph::types::RbdImageInfo`.
///
/// Not all fields from the original JSON-parsed struct are available via
/// librbd's `rbd_stat`. Fields that aren't available are set to sensible
/// defaults.
#[derive(Debug, Clone)]
pub struct RbdImageInfo {
    pub name: String,
    pub id: String,
    pub size: u64,
    pub objects: u64,
    pub order: u64,
    pub object_size: u64,
    pub snapshot_count: u64,
    pub block_name_prefix: String,
    pub format: u64,
    pub features: Vec<String>,
    pub op_features: Vec<String>,
    pub flags: Vec<String>,
    pub create_timestamp: String,
    pub access_timestamp: String,
    pub modify_timestamp: String,
    pub parent: Option<RbdImageParent>,
}

impl RbdImageInfo {
    pub fn size_mib(&self) -> u32 {
        (self.size / (1024 * 1024)) as u32
    }
}

/// Drop-in replacement for `ceph::types::RbdImageParent`.
#[derive(Debug, Clone)]
pub struct RbdImageParent {
    pub pool: String,
    pub pool_namespace: String,
    pub image: String,
    pub id: String,
    pub snapshot: String,
    pub trash: bool,
    pub overlap: u64,
}

impl From<crate::rbd::ImageInfo> for RbdImageInfo {
    fn from(info: crate::rbd::ImageInfo) -> Self {
        let parent = if !info.parent_name.is_empty() && info.parent_pool >= 0 {
            Some(RbdImageParent {
                pool: String::new(),
                pool_namespace: String::new(),
                image: info.parent_name.clone(),
                id: String::new(),
                snapshot: String::new(),
                trash: false,
                overlap: 0,
            })
        } else {
            None
        };

        RbdImageInfo {
            name: String::new(), // not available from rbd_stat
            id: String::new(),   // not available from rbd_stat
            size: info.size,
            objects: info.num_objs,
            order: info.order as u64,
            object_size: info.obj_size,
            snapshot_count: 0, // filled in separately when needed
            block_name_prefix: info.block_name_prefix,
            format: 2,
            features: vec![],
            op_features: vec![],
            flags: vec![],
            create_timestamp: String::new(),
            access_timestamp: String::new(),
            modify_timestamp: String::new(),
            parent,
        }
    }
}

// ---------------------------------------------------------------------------
// RbdClient — mirrors the old API, delegates to native Client
// ---------------------------------------------------------------------------

/// Drop-in replacement for `ceph::RbdClient`.
#[derive(Debug)]
pub struct RbdClient {
    inner: Client,
}

impl RbdClient {
    /// Create a new RBD client. The `timeout_duration` parameter is accepted
    /// for API compatibility but is ignored — native calls don't need it.
    pub fn new(
        id: String,
        pool_name: String,
        _timeout_duration: Duration,
    ) -> Result<Self, RbdClientError> {
        let inner = Client::connect(&id, &pool_name)?;
        Ok(Self { inner })
    }

    // -- Namespace operations --

    pub async fn namespace_create(&self, namespace: &str) -> Result<(), RbdClientError> {
        self.inner
            .namespace_create(namespace)
            .await
            .map_err(Into::into)
    }

    pub async fn namespace_exists(&self, namespace: &str) -> Result<bool, RbdClientError> {
        self.inner
            .namespace_exists(namespace)
            .await
            .map_err(Into::into)
    }

    pub async fn namespace_ensure(&self, namespace: &str) -> Result<(), RbdClientError> {
        self.inner
            .namespace_ensure(namespace)
            .await
            .map_err(Into::into)
    }

    // -- Image operations --

    pub async fn image_list(&self) -> Result<Vec<String>, RbdClientError> {
        self.inner.image_list().await.map_err(Into::into)
    }

    pub async fn image_create(
        &self,
        image_name: &str,
        size_mib: u32,
    ) -> Result<(), RbdClientError> {
        self.inner
            .image_create(image_name, size_mib)
            .await
            .map_err(Into::into)
    }

    pub async fn image_remove(&self, image_name: &str) -> Result<(), RbdClientError> {
        self.inner
            .image_remove(image_name)
            .await
            .map_err(Into::into)
    }

    pub async fn image_exists(&self, image_name: &str) -> Result<bool, RbdClientError> {
        self.inner
            .image_exists(image_name)
            .await
            .map_err(Into::into)
    }

    pub async fn image_info(&self, image_name: &str) -> Result<RbdImageInfo, RbdClientError> {
        let info = self.inner.image_info(image_name).await?;

        // Get snapshot count separately
        let snap_count = match self.inner.snap_list(image_name).await {
            Ok(snaps) => snaps.len() as u64,
            Err(_) => 0,
        };

        let mut rbd_info: RbdImageInfo = info.into();
        rbd_info.snapshot_count = snap_count;
        Ok(rbd_info)
    }

    pub async fn image_grow(&self, image_name: &str, size_mib: u32) -> Result<(), RbdClientError> {
        self.inner
            .image_grow(image_name, size_mib)
            .await
            .map_err(Into::into)
    }

    pub async fn image_has_watchers(&self, image_name: &str) -> Result<bool, RbdClientError> {
        self.inner
            .image_has_watchers(image_name)
            .await
            .map_err(Into::into)
    }

    // -- Snapshot operations (accept &RbdSnapName like the old API) --

    pub async fn snap_list(&self, image_name: &str) -> Result<Vec<RbdSnapName>, RbdClientError> {
        self.inner
            .snap_list_named(image_name)
            .await
            .map_err(Into::into)
    }

    pub async fn snap_create(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        self.inner
            .snap_create_named(snap_name)
            .await
            .map_err(Into::into)
    }

    pub async fn snap_remove(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        self.inner
            .snap_remove_named(snap_name)
            .await
            .map_err(Into::into)
    }

    pub async fn snap_protect(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        self.inner
            .snap_protect_named(snap_name)
            .await
            .map_err(Into::into)
    }

    pub async fn snap_unprotect(&self, snap_name: &RbdSnapName) -> Result<(), RbdClientError> {
        self.inner
            .snap_unprotect_named(snap_name)
            .await
            .map_err(Into::into)
    }

    pub async fn snap_clone(
        &self,
        snap_name: &RbdSnapName,
        new_image_name: &str,
    ) -> Result<(), RbdClientError> {
        self.inner
            .snap_clone_named(snap_name, new_image_name)
            .await
            .map_err(Into::into)
    }

    pub async fn snap_purge(&self, image_name: &str) -> Result<(), RbdClientError> {
        self.inner.snap_purge(image_name).await.map_err(Into::into)
    }

    pub async fn snap_has_children(&self, snap_name: &RbdSnapName) -> Result<bool, RbdClientError> {
        self.inner
            .snap_has_children_named(snap_name)
            .await
            .map_err(Into::into)
    }

    // -- Device operations --

    pub async fn device_map(&self, image_name: &str) -> Result<PathBuf, RbdClientError> {
        self.inner.device_map(image_name).await.map_err(Into::into)
    }

    pub async fn device_unmap<P: AsRef<Path>>(&self, device_path: P) -> Result<(), RbdClientError> {
        self.inner
            .device_unmap(device_path)
            .await
            .map_err(Into::into)
    }

    // -- Ceph diagnostic commands --

    pub async fn ceph_status(&self) -> Result<String, RbdClientError> {
        self.inner.ceph_status().await.map_err(Into::into)
    }

    pub async fn ceph_client_version(&self) -> Result<String, RbdClientError> {
        self.inner.ceph_client_version().await.map_err(Into::into)
    }

    pub async fn ceph_cluster_version(&self) -> Result<String, RbdClientError> {
        self.inner.ceph_cluster_version().await.map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// default_rbd_client() singleton — mirrors old ceph::default_rbd_client()
// ---------------------------------------------------------------------------

static DEFAULT_RBD_CLIENT: OnceLock<Result<RbdClient, RbdClientError>> = OnceLock::new();

/// Drop-in replacement for `ceph::default_rbd_client()`.
pub fn default_rbd_client() -> Result<&'static RbdClient, RbdClientError> {
    DEFAULT_RBD_CLIENT
        .get_or_init(|| {
            RbdClient::new(
                "chelsea".to_string(),
                "rbd".to_string(),
                Duration::from_secs(30),
            )
        })
        .as_ref()
        .map_err(|e| e.clone())
}
