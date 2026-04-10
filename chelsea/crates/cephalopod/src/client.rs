//! Async client wrapping the blocking FFI calls via `spawn_blocking`.
//!
//! This is the primary public API — a drop-in replacement for the old
//! exec-based `RbdClient`. It maintains a persistent cluster connection
//! and creates short-lived ioctx handles per operation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tracing::{debug, warn};

use crate::error::{CephalopodError, check_rc};
use crate::ffi::rbd as ffi;
use crate::handle_cache::HandleCache;
use crate::rados::{RadosCluster, RadosIoCtx};
use crate::rbd::{self, ChildInfo, ImageInfo, SnapInfo, WatcherInfo};
use crate::snap_name::RbdSnapName;

/// Parse a potentially namespaced image name like `"owner_id/my-image"`.
/// Returns `(Some("owner_id"), "my-image")` or `(None, "my-image")`.
fn parse_namespaced_image(image_name: &str) -> (Option<&str>, &str) {
    match image_name.rsplit_once('/') {
        Some((namespace, bare_name)) => (Some(namespace), bare_name),
        None => (None, image_name),
    }
}

/// A native async RBD client backed by librados/librbd FFI.
///
/// Replaces the old exec-based `RbdClient`. All operations use native
/// library calls. Device map/unmap use direct sysfs writes to the
/// kernel `krbd` module instead of shelling out.
///
/// The cluster connection is established once and shared across all
/// operations. Each operation creates a short-lived `rados_ioctx_t`.
pub struct Client {
    cluster: Arc<RadosCluster>,
    pool_name: String,
    /// Pre-parsed mon address for krbd sysfs writes (e.g. "172.16.0.2:6789")
    mon_addr: String,
    /// Client entity for krbd sysfs writes (e.g. "client.chelsea")
    entity: String,
    /// LRU cache of open rbd_image_t handles. rbd_open costs ~5.8ms (OSD
    /// roundtrip), but once open, rbd_stat is ~70ns. Caching handles gives
    /// ~83,000x speedup on repeated reads.
    handle_cache: HandleCache,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("pool_name", &self.pool_name)
            .field("mon_addr", &self.mon_addr)
            .field("entity", &self.entity)
            .finish()
    }
}

/// Parse the mon_host config value to extract a v1 (legacy msgr) address.
/// The kernel rbd module requires the legacy messenger format.
///
/// Input examples:
///   - `"[v2:172.16.0.2:3300/0,v1:172.16.0.2:6789/0]"` → `"172.16.0.2:6789"`
///   - `"172.16.0.2"` → `"172.16.0.2:6789"` (assumes default port)
///   - `"172.16.0.2:6789"` → `"172.16.0.2:6789"`
fn parse_mon_addr(mon_host: &str) -> Result<String, CephalopodError> {
    // Try to find v1: address in msgr2 format
    if let Some(v1_start) = mon_host.find("v1:") {
        let after_v1 = &mon_host[v1_start + 3..];
        // Extract addr:port, strip trailing /0] etc
        let addr = after_v1
            .split(|c: char| c == '/' || c == ']' || c == ',')
            .next()
            .unwrap_or(after_v1);
        return Ok(addr.to_string());
    }

    // Plain format — might be just an IP or IP:port
    let trimmed = mon_host.trim().trim_matches(|c| c == '[' || c == ']');
    if trimmed.contains(':') {
        Ok(trimmed.to_string())
    } else {
        // Assume default legacy port
        Ok(format!("{trimmed}:6789"))
    }
}

impl Client {
    /// Connect to Ceph as the given client id (e.g. "chelsea") and bind to a pool.
    pub fn connect(id: &str, pool_name: &str) -> Result<Self, CephalopodError> {
        let cluster = RadosCluster::connect(id)?;

        // Read mon_host for krbd sysfs operations
        let mon_host = cluster.conf_get("mon_host")?;
        let mon_addr = parse_mon_addr(&mon_host)?;
        let entity = format!("client.{id}");

        Ok(Self {
            cluster: Arc::new(cluster),
            pool_name: pool_name.to_string(),
            mon_addr,
            entity,
            // Cache up to 4 idle handles per image, 128 total
            handle_cache: HandleCache::new(4, 128),
        })
    }

    /// Get an ioctx for the pool, optionally with a namespace set.
    fn ioctx(&self, namespace: Option<&str>) -> Result<RadosIoCtx, CephalopodError> {
        let ioctx = self.cluster.ioctx(&self.pool_name)?;
        if let Some(ns) = namespace {
            ioctx.set_namespace(ns)?;
        }
        Ok(ioctx)
    }

    /// Get an ioctx with namespace parsed from a potentially namespaced image name.
    /// Returns `(ioctx, bare_image_name)`.
    fn ioctx_for_image<'a>(
        &self,
        image_name: &'a str,
    ) -> Result<(RadosIoCtx, &'a str), CephalopodError> {
        let (ns, bare) = parse_namespaced_image(image_name);
        let ioctx = self.ioctx(ns)?;
        Ok((ioctx, bare))
    }

    /// Execute a read operation using a cached image handle. The entire
    /// checkout → operation → checkin cycle runs on the blocking threadpool
    /// to avoid Send issues with raw pointers.
    async fn with_cached_handle<F, T>(
        &self,
        ns: Option<&str>,
        bare_image: &str,
        op: F,
    ) -> Result<T, CephalopodError>
    where
        F: FnOnce(ffi::rbd_image_t) -> Result<T, CephalopodError> + Send + 'static,
        T: Send + 'static,
    {
        let ioctx = self.ioctx(ns)?;
        let ns_owned = ns.map(|s| s.to_string());
        let bare_owned = bare_image.to_string();

        // Extract raw pointers that are Send-safe to pass to spawn_blocking.
        // SAFETY: Both pointers remain valid for the duration of the task:
        // - cache: lives as long as Client (&self)
        // - ioctx: lives on our stack until the task completes (we .await it)
        let cache_raw = &self.handle_cache as *const HandleCache as usize;
        let ioctx_raw = ioctx.raw() as usize;

        let result = tokio::task::spawn_blocking(move || {
            let cache = unsafe { &*(cache_raw as *const HandleCache) };
            let ioctx_ptr = ioctx_raw as crate::ffi::rados::rados_ioctx_t;

            let handle = cache.checkout_raw(ioctx_ptr, ns_owned.as_deref(), &bare_owned)?;
            let raw = handle.raw();

            let result = op(raw);

            match &result {
                Ok(_) => cache.checkin(ns_owned.as_deref(), &bare_owned, handle),
                Err(_) => drop(handle),
            }

            result
        })
        .await
        .expect("spawn_blocking join");

        // Keep ioctx alive until the blocking task completes
        drop(ioctx);

        result
    }

    /// Execute a mutation using a cached handle. The handle is NOT returned
    /// to the cache after — instead, all cached handles for the image are
    /// invalidated (since metadata like snap lists will have changed).
    async fn with_cached_handle_mut<F, T>(
        &self,
        ns: Option<&str>,
        bare_image: &str,
        op: F,
    ) -> Result<T, CephalopodError>
    where
        F: FnOnce(ffi::rbd_image_t) -> Result<T, CephalopodError> + Send + 'static,
        T: Send + 'static,
    {
        let ioctx = self.ioctx(ns)?;
        let ns_owned = ns.map(|s| s.to_string());
        let bare_owned = bare_image.to_string();

        let cache_raw = &self.handle_cache as *const HandleCache as usize;
        let ioctx_raw = ioctx.raw() as usize;

        let result = tokio::task::spawn_blocking(move || {
            let cache = unsafe { &*(cache_raw as *const HandleCache) };
            let ioctx_ptr = ioctx_raw as crate::ffi::rados::rados_ioctx_t;

            let handle = cache.checkout_raw(ioctx_ptr, ns_owned.as_deref(), &bare_owned)?;
            let raw = handle.raw();

            let result = op(raw);

            // Always close this handle (don't return to cache)
            drop(handle);
            // Invalidate any remaining cached handles — metadata has changed
            cache.invalidate(ns_owned.as_deref(), &bare_owned);

            result
        })
        .await
        .expect("spawn_blocking join");

        drop(ioctx);
        result
    }

    /// Invalidate cached handles for an image after a mutation.
    fn invalidate_image_cache(&self, image_name: &str) {
        let (ns, bare) = parse_namespaced_image(image_name);
        self.handle_cache.invalidate(ns, bare);
    }

    // -----------------------------------------------------------------------
    // Namespace operations
    // -----------------------------------------------------------------------

    /// Create a namespace in the pool.
    pub async fn namespace_create(&self, namespace: &str) -> Result<(), CephalopodError> {
        debug!(%namespace, pool_name = %self.pool_name, "Creating RBD namespace");
        let ioctx = self.ioctx(None)?;
        let namespace = namespace.to_string();
        tokio::task::spawn_blocking(move || rbd::namespace_create(&ioctx, &namespace))
            .await
            .expect("spawn_blocking join")
    }

    /// Check if a namespace exists.
    pub async fn namespace_exists(&self, namespace: &str) -> Result<bool, CephalopodError> {
        debug!(%namespace, pool_name = %self.pool_name, "Checking if RBD namespace exists");
        let ioctx = self.ioctx(None)?;
        let namespace = namespace.to_string();
        tokio::task::spawn_blocking(move || rbd::namespace_exists(&ioctx, &namespace))
            .await
            .expect("spawn_blocking join")
    }

    /// List all namespaces in the pool.
    pub async fn namespace_list(&self) -> Result<Vec<String>, CephalopodError> {
        let ioctx = self.ioctx(None)?;
        tokio::task::spawn_blocking(move || rbd::namespace_list(&ioctx))
            .await
            .expect("spawn_blocking join")
    }

    /// Ensure a namespace exists, creating it if necessary. Idempotent.
    pub async fn namespace_ensure(&self, namespace: &str) -> Result<(), CephalopodError> {
        if !self.namespace_exists(namespace).await? {
            match self.namespace_create(namespace).await {
                Ok(()) => {
                    debug!(%namespace, "Created RBD namespace");
                    Ok(())
                }
                Err(CephalopodError::AlreadyExists(_)) => {
                    debug!(%namespace, "RBD namespace already exists (race condition)");
                    Ok(())
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Image operations
    // -----------------------------------------------------------------------

    /// List all images in the pool (default namespace).
    pub async fn image_list(&self) -> Result<Vec<String>, CephalopodError> {
        let ioctx = self.ioctx(None)?;
        tokio::task::spawn_blocking(move || rbd::image_list(&ioctx))
            .await
            .expect("spawn_blocking join")
    }

    /// Create an image with the given size in MiB.
    /// Image name may include a namespace prefix (e.g. "owner_id/my-image").
    pub async fn image_create(
        &self,
        image_name: &str,
        size_mib: u32,
    ) -> Result<(), CephalopodError> {
        debug!(%image_name, %size_mib, "Creating RBD image");
        let (ioctx, bare) = self.ioctx_for_image(image_name)?;
        let bare = bare.to_string();
        tokio::task::spawn_blocking(move || rbd::image_create(&ioctx, &bare, size_mib))
            .await
            .expect("spawn_blocking join")
    }

    /// Remove an image. Image must have no snapshots.
    /// Image name may include a namespace prefix.
    pub async fn image_remove(&self, image_name: &str) -> Result<(), CephalopodError> {
        debug!(%image_name, pool_name = %self.pool_name, "Removing RBD image");
        self.invalidate_image_cache(image_name);
        let (ioctx, bare) = self.ioctx_for_image(image_name)?;
        let bare = bare.to_string();
        tokio::task::spawn_blocking(move || rbd::image_remove(&ioctx, &bare))
            .await
            .expect("spawn_blocking join")
    }

    /// Check if an image exists. Uses cached handles — if we have a cached
    /// handle, we know it exists without any network call.
    /// Image name may include a namespace prefix.
    pub async fn image_exists(&self, image_name: &str) -> Result<bool, CephalopodError> {
        debug!(%image_name, "Checking if RBD image exists");
        let (ns, bare) = parse_namespaced_image(image_name);

        // Try the cache first — if we get a handle, image exists
        match self.with_cached_handle(ns, bare, |_| Ok(())).await {
            Ok(()) => Ok(true),
            Err(CephalopodError::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Get image info (stat). Uses cached handles for ~83,000x speedup on
    /// repeated calls to the same image.
    /// Image name may include a namespace prefix.
    pub async fn image_info(&self, image_name: &str) -> Result<ImageInfo, CephalopodError> {
        let (ns, bare) = parse_namespaced_image(image_name);
        let bare_owned = bare.to_string();

        self.with_cached_handle(ns, bare, move |handle| {
            let mut info: ffi::rbd_image_info_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                ffi::rbd_stat(
                    handle,
                    &mut info,
                    std::mem::size_of::<ffi::rbd_image_info_t>(),
                )
            };
            check_rc(rc, format!("rbd_stat({bare_owned})"))?;

            let prefix = unsafe { std::ffi::CStr::from_ptr(info.block_name_prefix.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            let parent = unsafe { std::ffi::CStr::from_ptr(info.parent_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();

            Ok(ImageInfo {
                size: info.size,
                obj_size: info.obj_size,
                num_objs: info.num_objs,
                order: info.order,
                block_name_prefix: prefix,
                parent_pool: info.parent_pool,
                parent_name: parent,
            })
        })
        .await
    }

    /// Check if an image has watchers (i.e. is mapped/in use).
    /// Uses cached handles.
    pub async fn image_has_watchers(&self, image_name: &str) -> Result<bool, CephalopodError> {
        let (ns, bare) = parse_namespaced_image(image_name);
        self.with_cached_handle(ns, bare, |handle| {
            let mut max_watchers: usize = 16;
            let mut watchers: Vec<ffi::rbd_image_watcher_t> = vec![
                ffi::rbd_image_watcher_t {
                    addr: std::ptr::null_mut(),
                    id: 0,
                    cookie: 0,
                };
                max_watchers
            ];
            let rc =
                unsafe { ffi::rbd_watchers_list(handle, watchers.as_mut_ptr(), &mut max_watchers) };
            check_rc(rc, "rbd_watchers_list")?;
            let has = max_watchers > 0;
            unsafe { ffi::rbd_watchers_list_cleanup(watchers.as_mut_ptr(), max_watchers) };
            Ok(has)
        })
        .await
    }

    /// Get watchers on an image. Uses cached handles.
    pub async fn image_watchers(
        &self,
        image_name: &str,
    ) -> Result<Vec<WatcherInfo>, CephalopodError> {
        let (ns, bare) = parse_namespaced_image(image_name);
        self.with_cached_handle(ns, bare, |handle| {
            let mut max_watchers: usize = 16;
            let mut watchers: Vec<ffi::rbd_image_watcher_t> = vec![
                ffi::rbd_image_watcher_t {
                    addr: std::ptr::null_mut(),
                    id: 0,
                    cookie: 0,
                };
                max_watchers
            ];
            let rc =
                unsafe { ffi::rbd_watchers_list(handle, watchers.as_mut_ptr(), &mut max_watchers) };
            check_rc(rc, "rbd_watchers_list")?;
            let result: Vec<WatcherInfo> = watchers[..max_watchers]
                .iter()
                .map(|w| {
                    let addr = if w.addr.is_null() {
                        String::new()
                    } else {
                        unsafe { std::ffi::CStr::from_ptr(w.addr) }
                            .to_string_lossy()
                            .into_owned()
                    };
                    WatcherInfo {
                        addr,
                        id: w.id,
                        cookie: w.cookie,
                    }
                })
                .collect();
            unsafe { ffi::rbd_watchers_list_cleanup(watchers.as_mut_ptr(), max_watchers) };
            Ok(result)
        })
        .await
    }

    /// Grow an image to the given size in MiB. Skips if already at that size.
    /// Uses a single cached handle for both the size check and the resize.
    pub async fn image_grow(&self, image_name: &str, size_mib: u32) -> Result<(), CephalopodError> {
        let (ns, bare) = parse_namespaced_image(image_name);
        let bare_owned = bare.to_string();
        let target_bytes = (size_mib as u64) * 1024 * 1024;

        self.with_cached_handle_mut(ns, bare, move |handle| {
            // Check current size
            let mut current_size: u64 = 0;
            let rc = unsafe { ffi::rbd_get_size(handle, &mut current_size) };
            check_rc(rc, format!("rbd_get_size({bare_owned})"))?;

            if current_size == target_bytes {
                debug!(%bare_owned, %size_mib, "Skipping RBD grow; new size is equal to original");
                return Ok(());
            }

            debug!(%bare_owned, %size_mib, "Growing RBD image size");
            let rc = unsafe {
                ffi::rbd_resize2(handle, target_bytes, false, None, std::ptr::null_mut())
            };
            check_rc(rc, format!("rbd_resize2({bare_owned}, {target_bytes})"))?;
            Ok(())
        })
        .await
    }

    // -----------------------------------------------------------------------
    // Snapshot operations
    // -----------------------------------------------------------------------

    /// List snapshots on an image. Uses cached handles.
    /// Image name may include a namespace prefix.
    pub async fn snap_list(&self, image_name: &str) -> Result<Vec<SnapInfo>, CephalopodError> {
        let (ns, bare) = parse_namespaced_image(image_name);
        self.with_cached_handle(ns, bare, |handle| {
            use std::os::raw::c_int;
            let mut max_snaps: c_int = 64;
            let mut snaps: Vec<ffi::rbd_snap_info_t> = vec![
                ffi::rbd_snap_info_t {
                    id: 0,
                    size: 0,
                    name: std::ptr::null(),
                };
                max_snaps as usize
            ];
            let rc = unsafe { ffi::rbd_snap_list(handle, snaps.as_mut_ptr(), &mut max_snaps) };
            if rc < 0 {
                check_rc(rc, "rbd_snap_list")?;
            }
            let count = rc as usize;
            let result = snaps[..count]
                .iter()
                .map(|s| {
                    let name = if s.name.is_null() {
                        String::new()
                    } else {
                        unsafe { std::ffi::CStr::from_ptr(s.name) }
                            .to_string_lossy()
                            .into_owned()
                    };
                    SnapInfo {
                        id: s.id,
                        name,
                        size: s.size,
                    }
                })
                .collect();
            unsafe { ffi::rbd_snap_list_end(snaps.as_mut_ptr()) };
            Ok(result)
        })
        .await
    }

    /// Create a snapshot. Uses cached handle (mutation — invalidates cache after).
    /// Image name may include a namespace prefix.
    pub async fn snap_create(
        &self,
        image_name: &str,
        snap_name: &str,
    ) -> Result<(), CephalopodError> {
        debug!(%image_name, %snap_name, "Creating RBD snapshot");
        let (ns, bare) = parse_namespaced_image(image_name);
        let snap = snap_name.to_string();
        self.with_cached_handle_mut(ns, bare, move |handle| {
            let c_snap = std::ffi::CString::new(snap.as_str())?;
            let rc = unsafe { ffi::rbd_snap_create(handle, c_snap.as_ptr()) };
            check_rc(rc, format!("rbd_snap_create({snap})"))?;
            Ok(())
        })
        .await
    }

    /// Remove a snapshot. Must not be protected.
    /// Image name may include a namespace prefix.
    pub async fn snap_remove(
        &self,
        image_name: &str,
        snap_name: &str,
    ) -> Result<(), CephalopodError> {
        debug!(%image_name, %snap_name, "Removing RBD snapshot");
        let (ns, bare) = parse_namespaced_image(image_name);
        let snap = snap_name.to_string();
        self.with_cached_handle_mut(ns, bare, move |handle| {
            let c_snap = std::ffi::CString::new(snap.as_str())?;
            let rc = unsafe { ffi::rbd_snap_remove(handle, c_snap.as_ptr()) };
            check_rc(rc, format!("rbd_snap_remove({snap})"))?;
            Ok(())
        })
        .await
    }

    /// Protect a snapshot. Uses cached handle (mutation).
    /// Image name may include a namespace prefix.
    pub async fn snap_protect(
        &self,
        image_name: &str,
        snap_name: &str,
    ) -> Result<(), CephalopodError> {
        debug!(%image_name, %snap_name, "Protecting RBD snapshot");
        let (ns, bare) = parse_namespaced_image(image_name);
        let snap = snap_name.to_string();
        self.with_cached_handle_mut(ns, bare, move |handle| {
            let c_snap = std::ffi::CString::new(snap.as_str())?;
            let rc = unsafe { ffi::rbd_snap_protect(handle, c_snap.as_ptr()) };
            check_rc(rc, format!("rbd_snap_protect({snap})"))?;
            Ok(())
        })
        .await
    }

    /// Unprotect a snapshot. Uses cached handle (mutation).
    /// Image name may include a namespace prefix.
    pub async fn snap_unprotect(
        &self,
        image_name: &str,
        snap_name: &str,
    ) -> Result<(), CephalopodError> {
        debug!(%image_name, %snap_name, "Unprotecting RBD snapshot");
        let (ns, bare) = parse_namespaced_image(image_name);
        let snap = snap_name.to_string();
        self.with_cached_handle_mut(ns, bare, move |handle| {
            let c_snap = std::ffi::CString::new(snap.as_str())?;
            let rc = unsafe { ffi::rbd_snap_unprotect(handle, c_snap.as_ptr()) };
            check_rc(rc, format!("rbd_snap_unprotect({snap})"))?;
            Ok(())
        })
        .await
    }

    /// Create and protect a snapshot in a single operation. This saves one
    /// rbd_open + rbd_close cycle (~5.8ms) and one spawn_blocking dispatch
    /// (~6µs) compared to calling snap_create then snap_protect separately.
    pub async fn snap_create_and_protect(
        &self,
        image_name: &str,
        snap_name: &str,
    ) -> Result<(), CephalopodError> {
        debug!(%image_name, %snap_name, "Creating and protecting RBD snapshot (combined)");
        let (ns, bare) = parse_namespaced_image(image_name);
        let snap = snap_name.to_string();
        self.with_cached_handle_mut(ns, bare, move |handle| {
            let c_snap = std::ffi::CString::new(snap.as_str())?;
            let rc = unsafe { ffi::rbd_snap_create(handle, c_snap.as_ptr()) };
            check_rc(rc, format!("rbd_snap_create({snap})"))?;
            let rc = unsafe { ffi::rbd_snap_protect(handle, c_snap.as_ptr()) };
            check_rc(rc, format!("rbd_snap_protect({snap})"))?;
            Ok(())
        })
        .await
    }

    /// Check if a snapshot has any clone-children.
    /// Image name may include a namespace prefix.
    pub async fn snap_has_children(
        &self,
        image_name: &str,
        snap_name: &str,
    ) -> Result<bool, CephalopodError> {
        debug!(%image_name, %snap_name, "Checking if RBD snapshot has children");
        let (ioctx, bare) = self.ioctx_for_image(image_name)?;
        let bare = bare.to_string();
        let snap = snap_name.to_string();
        let children =
            tokio::task::spawn_blocking(move || rbd::snap_list_children(&ioctx, &bare, &snap))
                .await
                .expect("spawn_blocking join")?;
        Ok(!children.is_empty())
    }

    /// List children (clones) of a snapshot.
    /// Image name may include a namespace prefix.
    pub async fn snap_list_children(
        &self,
        image_name: &str,
        snap_name: &str,
    ) -> Result<Vec<ChildInfo>, CephalopodError> {
        let (ioctx, bare) = self.ioctx_for_image(image_name)?;
        let bare = bare.to_string();
        let snap = snap_name.to_string();
        tokio::task::spawn_blocking(move || rbd::snap_list_children(&ioctx, &bare, &snap))
            .await
            .expect("spawn_blocking join")
    }

    /// Purge all snapshots on an image. Uses a single handle for the entire
    /// list→unprotect→remove loop, saving N*5.8ms of rbd_open overhead.
    /// Image name may include a namespace prefix.
    pub async fn snap_purge(&self, image_name: &str) -> Result<(), CephalopodError> {
        debug!(%image_name, "Purging all RBD snapshots for image");
        let (ns, bare) = parse_namespaced_image(image_name);
        self.with_cached_handle_mut(ns, bare, move |handle| {
            use std::os::raw::c_int;

            // List all snaps
            let mut max_snaps: c_int = 64;
            let mut snaps: Vec<ffi::rbd_snap_info_t> = vec![
                ffi::rbd_snap_info_t {
                    id: 0,
                    size: 0,
                    name: std::ptr::null(),
                };
                max_snaps as usize
            ];
            let rc = unsafe { ffi::rbd_snap_list(handle, snaps.as_mut_ptr(), &mut max_snaps) };
            if rc < 0 {
                check_rc(rc, "rbd_snap_list")?;
            }
            let count = rc as usize;

            // Collect snap names before cleanup
            let snap_names: Vec<String> = snaps[..count]
                .iter()
                .map(|s| {
                    if s.name.is_null() {
                        String::new()
                    } else {
                        unsafe { std::ffi::CStr::from_ptr(s.name) }
                            .to_string_lossy()
                            .into_owned()
                    }
                })
                .collect();
            unsafe { ffi::rbd_snap_list_end(snaps.as_mut_ptr()) };

            // Unprotect + remove each snap
            for snap_name in &snap_names {
                let c_snap = std::ffi::CString::new(snap_name.as_str())?;

                // Try to unprotect (might not be protected)
                let mut is_protected: c_int = 0;
                let rc = unsafe {
                    ffi::rbd_snap_is_protected(handle, c_snap.as_ptr(), &mut is_protected)
                };
                check_rc(rc, format!("rbd_snap_is_protected({snap_name})"))?;

                if is_protected != 0 {
                    let rc = unsafe { ffi::rbd_snap_unprotect(handle, c_snap.as_ptr()) };
                    check_rc(rc, format!("rbd_snap_unprotect({snap_name})"))?;
                }

                let rc = unsafe { ffi::rbd_snap_remove(handle, c_snap.as_ptr()) };
                check_rc(rc, format!("rbd_snap_remove({snap_name})"))?;
            }

            Ok(())
        })
        .await
    }

    /// Clone a snapshot to a new image. Uses clone format v2.
    ///
    /// The source image name may include a namespace prefix (e.g. "owner_id/base-image").
    /// The destination image name may also include a namespace prefix, or be in the
    /// default namespace.
    pub async fn snap_clone(
        &self,
        src_image_name: &str,
        src_snap_name: &str,
        dst_image_name: &str,
    ) -> Result<(), CephalopodError> {
        debug!(%src_image_name, %src_snap_name, %dst_image_name, "Cloning RBD snapshot (format v2)");

        let (src_ns, src_bare) = parse_namespaced_image(src_image_name);
        let (dst_ns, dst_bare) = parse_namespaced_image(dst_image_name);

        let src_ioctx = self.ioctx(src_ns)?;
        let dst_ioctx = self.ioctx(dst_ns)?;

        let src_bare = src_bare.to_string();
        let src_snap = src_snap_name.to_string();
        let dst_bare = dst_bare.to_string();

        tokio::task::spawn_blocking(move || {
            rbd::snap_clone(&src_ioctx, &src_bare, &src_snap, &dst_ioctx, &dst_bare)
        })
        .await
        .expect("spawn_blocking join")
    }

    // -----------------------------------------------------------------------
    // RbdSnapName convenience methods
    //
    // These mirror the string-based methods above but accept `RbdSnapName`
    // for compatibility with existing callers.
    // -----------------------------------------------------------------------

    /// Create a snapshot (RbdSnapName variant).
    pub async fn snap_create_named(&self, snap: &RbdSnapName) -> Result<(), CephalopodError> {
        self.snap_create(&snap.image_name, &snap.snap_name).await
    }

    /// Remove a snapshot (RbdSnapName variant).
    pub async fn snap_remove_named(&self, snap: &RbdSnapName) -> Result<(), CephalopodError> {
        self.snap_remove(&snap.image_name, &snap.snap_name).await
    }

    /// Protect a snapshot (RbdSnapName variant).
    pub async fn snap_protect_named(&self, snap: &RbdSnapName) -> Result<(), CephalopodError> {
        self.snap_protect(&snap.image_name, &snap.snap_name).await
    }

    /// Unprotect a snapshot (RbdSnapName variant).
    pub async fn snap_unprotect_named(&self, snap: &RbdSnapName) -> Result<(), CephalopodError> {
        self.snap_unprotect(&snap.image_name, &snap.snap_name).await
    }

    /// Check if a snapshot has children (RbdSnapName variant).
    pub async fn snap_has_children_named(
        &self,
        snap: &RbdSnapName,
    ) -> Result<bool, CephalopodError> {
        self.snap_has_children(&snap.image_name, &snap.snap_name)
            .await
    }

    /// Clone a snapshot to a new image (RbdSnapName variant).
    pub async fn snap_clone_named(
        &self,
        snap: &RbdSnapName,
        dst_image_name: &str,
    ) -> Result<(), CephalopodError> {
        self.snap_clone(&snap.image_name, &snap.snap_name, dst_image_name)
            .await
    }

    /// List snapshots, returning RbdSnapName structs.
    pub async fn snap_list_named(
        &self,
        image_name: &str,
    ) -> Result<Vec<RbdSnapName>, CephalopodError> {
        let snaps = self.snap_list(image_name).await?;
        Ok(snaps
            .into_iter()
            .map(|s| RbdSnapName {
                image_name: image_name.to_string(),
                snap_name: s.name,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Device operations (native sysfs writes to kernel krbd module)
    // -----------------------------------------------------------------------

    /// Map an RBD image to a block device via `/sys/bus/rbd/add_single_major`.
    /// Returns the device path (e.g. `/dev/rbd0`).
    ///
    /// Image name may include a namespace prefix (e.g. "owner_id/my-image").
    pub async fn device_map(&self, image_name: &str) -> Result<PathBuf, CephalopodError> {
        debug!(%image_name, pool_name = %self.pool_name, "Mapping RBD image to device");

        let (ns, bare) = parse_namespaced_image(image_name);

        // Build the sysfs add string:
        // "<mon_addr> name=<user>,key=<entity>[,_pool_ns=<ns>] <pool> <image> -"
        let options = match ns {
            Some(namespace) => format!(
                "name={},key={},_pool_ns={}",
                self.cluster.id(),
                self.entity,
                namespace
            ),
            None => format!("name={},key={}", self.cluster.id(), self.entity),
        };
        let add_str = format!(
            "{} {} {} {} -",
            self.mon_addr, options, self.pool_name, bare
        );

        // Record existing device IDs before the add
        let before = Self::list_rbd_device_ids()?;

        // Write to sysfs — this is a synchronous kernel operation that completes
        // immediately (the kernel maps the device inline). We use spawn_blocking
        // to avoid blocking the async runtime.
        let add_str_clone = add_str.clone();
        tokio::task::spawn_blocking(move || {
            std::fs::write("/sys/bus/rbd/add_single_major", add_str_clone.as_bytes())
        })
        .await
        .expect("spawn_blocking join")
        .map_err(|e| {
            CephalopodError::Device(format!(
                "failed to write to /sys/bus/rbd/add_single_major: {e}"
            ))
        })?;

        // Find the new device ID by diffing
        let after = Self::list_rbd_device_ids()?;
        let new_id = after
            .into_iter()
            .find(|id| !before.contains(id))
            .ok_or_else(|| {
                CephalopodError::Device(
                    "no new device appeared in /sys/bus/rbd/devices/ after add".into(),
                )
            })?;

        let device_path = PathBuf::from(format!("/dev/rbd{new_id}"));
        debug!(%image_name, ?device_path, "Mapped RBD image to device");
        Ok(device_path)
    }

    /// Unmap an RBD device via `/sys/bus/rbd/remove_single_major`.
    /// Retries with exponential backoff on EBUSY.
    pub async fn device_unmap<P: AsRef<Path>>(
        &self,
        device_path: P,
    ) -> Result<(), CephalopodError> {
        debug!(device_path = ?device_path.as_ref(), "Unmapping RBD device");

        // Extract device ID from path: /dev/rbd0 → "0", /dev/rbd12 → "12"
        let dev_name = device_path
            .as_ref()
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                CephalopodError::Device(format!(
                    "invalid device path: {}",
                    device_path.as_ref().display()
                ))
            })?;
        let dev_id = dev_name
            .strip_prefix("rbd")
            .ok_or_else(|| CephalopodError::Device(format!("not an rbd device: {dev_name}")))?;

        const MAX_ATTEMPTS: u8 = 8;
        const INITIAL_DELAY: Duration = Duration::from_millis(200);
        const MAX_DELAY: Duration = Duration::from_secs(5);

        let mut last_err = None;
        let mut delay = INITIAL_DELAY;

        for attempt in 0..MAX_ATTEMPTS {
            let dev_id_clone = dev_id.to_string();
            let result = tokio::task::spawn_blocking(move || {
                std::fs::write("/sys/bus/rbd/remove_single_major", dev_id_clone.as_bytes())
            })
            .await
            .expect("spawn_blocking join");

            match result {
                Ok(()) => return Ok(()),
                Err(e) => {
                    let is_busy = e.raw_os_error() == Some(16); // EBUSY
                    if is_busy && attempt < MAX_ATTEMPTS - 1 {
                        warn!(
                            attempt,
                            max_attempts = MAX_ATTEMPTS,
                            delay_ms = delay.as_millis(),
                            device_path = ?device_path.as_ref(),
                            "rbd device unmap failed due to busy device; retrying"
                        );
                        last_err = Some(CephalopodError::Device(format!("device busy: {e}")));
                        sleep(delay).await;
                        delay = (delay * 2).min(MAX_DELAY);
                    } else {
                        return Err(CephalopodError::Device(format!(
                            "failed to write to /sys/bus/rbd/remove_single_major: {e}"
                        )));
                    }
                }
            }
        }
        Err(last_err
            .unwrap_or_else(|| CephalopodError::Device("device unmap failed repeatedly".into())))
    }

    // -----------------------------------------------------------------------
    // Ceph diagnostic commands (exec-based — infrequent diagnostic calls)
    // -----------------------------------------------------------------------

    /// Run `ceph --user <id> status` and return the output.
    pub async fn ceph_status(&self) -> Result<String, CephalopodError> {
        self.exec_ceph(&["status"]).await
    }

    /// Run `ceph --user <id> --version` and return the client version string.
    pub async fn ceph_client_version(&self) -> Result<String, CephalopodError> {
        self.exec_ceph(&["--version"]).await
    }

    /// Run `ceph --user <id> version` and return the cluster version string.
    pub async fn ceph_cluster_version(&self) -> Result<String, CephalopodError> {
        self.exec_ceph(&["version"]).await
    }

    /// Execute a `ceph` CLI command. Used only for diagnostic commands
    /// that don't have a clean librados equivalent.
    async fn exec_ceph(&self, args: &[&str]) -> Result<String, CephalopodError> {
        let output = tokio::process::Command::new("ceph")
            .arg("--user")
            .arg(self.cluster.id())
            .args(args)
            .output()
            .await
            .map_err(|e| CephalopodError::Device(format!("failed to exec ceph: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CephalopodError::Device(format!(
                "ceph command failed: {stderr}"
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// List current RBD device IDs from /sys/bus/rbd/devices/.
    fn list_rbd_device_ids() -> Result<Vec<String>, CephalopodError> {
        let entries = std::fs::read_dir("/sys/bus/rbd/devices").map_err(|e| {
            CephalopodError::Device(format!("failed to read /sys/bus/rbd/devices: {e}"))
        })?;
        let mut ids = Vec::new();
        for entry in entries {
            if let Ok(entry) = entry {
                if let Some(name) = entry.file_name().to_str() {
                    ids.push(name.to_string());
                }
            }
        }
        Ok(ids)
    }
}
