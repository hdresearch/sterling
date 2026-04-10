//! Safe wrappers around librbd: image and snapshot operations.
//!
//! All functions here are **blocking** (they call into librbd synchronously).
//! The async [`Client`](crate::client::Client) wraps these in `spawn_blocking`.

use std::ffi::{CStr, CString};
use std::mem;
use std::os::raw::c_int;
use std::ptr;

use crate::error::{CephalopodError, check_rc};
use crate::ffi::rbd;
use crate::rados::RadosIoCtx;

// ---------------------------------------------------------------------------
// Image handle
// ---------------------------------------------------------------------------

/// A handle to an open RBD image. Closes automatically on drop.
pub struct RbdImage {
    handle: rbd::rbd_image_t,
}

// SAFETY: rbd_image_t can be sent between threads (not used concurrently).
unsafe impl Send for RbdImage {}

impl RbdImage {
    /// Open an image by name (no snapshot).
    pub fn open(ioctx: &RadosIoCtx, name: &str) -> Result<Self, CephalopodError> {
        let c_name = CString::new(name)?;
        let mut handle: rbd::rbd_image_t = ptr::null_mut();

        let rc = unsafe { rbd::rbd_open(ioctx.raw(), c_name.as_ptr(), &mut handle, ptr::null()) };
        check_rc(rc, format!("rbd_open({name})"))?;
        Ok(Self { handle })
    }

    /// Open an image at a specific snapshot.
    pub fn open_at_snap(
        ioctx: &RadosIoCtx,
        name: &str,
        snap_name: &str,
    ) -> Result<Self, CephalopodError> {
        let c_name = CString::new(name)?;
        let c_snap = CString::new(snap_name)?;
        let mut handle: rbd::rbd_image_t = ptr::null_mut();

        let rc =
            unsafe { rbd::rbd_open(ioctx.raw(), c_name.as_ptr(), &mut handle, c_snap.as_ptr()) };
        check_rc(rc, format!("rbd_open({name}@{snap_name})"))?;
        Ok(Self { handle })
    }

    pub(crate) fn raw(&self) -> rbd::rbd_image_t {
        self.handle
    }
}

impl Drop for RbdImage {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { rbd::rbd_close(self.handle) };
        }
    }
}

// ---------------------------------------------------------------------------
// Image options helper
// ---------------------------------------------------------------------------

/// RAII wrapper for `rbd_image_options_t`.
struct ImageOptions {
    opts: rbd::rbd_image_options_t,
}

impl ImageOptions {
    fn new() -> Self {
        let mut opts: rbd::rbd_image_options_t = ptr::null_mut();
        unsafe { rbd::rbd_image_options_create(&mut opts) };
        Self { opts }
    }

    fn set_u64(&mut self, key: c_int, val: u64) -> Result<(), CephalopodError> {
        let rc = unsafe { rbd::rbd_image_options_set_uint64(self.opts, key, val) };
        check_rc(rc, "rbd_image_options_set_uint64")
    }

    fn raw(&self) -> rbd::rbd_image_options_t {
        self.opts
    }
}

impl Drop for ImageOptions {
    fn drop(&mut self) {
        unsafe { rbd::rbd_image_options_destroy(self.opts) };
    }
}

// ---------------------------------------------------------------------------
// Return types
// ---------------------------------------------------------------------------

/// Info about an RBD image, returned by [`image_stat`].
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub size: u64,
    pub obj_size: u64,
    pub num_objs: u64,
    pub order: i32,
    pub block_name_prefix: String,
    pub parent_pool: i64,
    pub parent_name: String,
}

impl ImageInfo {
    pub fn size_mib(&self) -> u32 {
        (self.size / (1024 * 1024)) as u32
    }
}

/// Info about a snapshot, returned by [`snap_list`].
#[derive(Debug, Clone)]
pub struct SnapInfo {
    pub id: u64,
    pub size: u64,
    pub name: String,
}

/// Info about a child image (clone), returned by [`snap_list_children`].
#[derive(Debug, Clone)]
pub struct ChildInfo {
    pub pool_id: i64,
    pub pool_name: String,
    pub pool_namespace: String,
    pub image_id: String,
    pub image_name: String,
    pub trash: bool,
}

/// Info about an image watcher, returned by [`image_watchers`].
#[derive(Debug, Clone)]
pub struct WatcherInfo {
    pub addr: String,
    pub id: i64,
    pub cookie: u64,
}

// ---------------------------------------------------------------------------
// Namespace operations (take ioctx, not image handle)
// ---------------------------------------------------------------------------

pub fn namespace_create(ioctx: &RadosIoCtx, namespace: &str) -> Result<(), CephalopodError> {
    let c_ns = CString::new(namespace)?;
    let rc = unsafe { rbd::rbd_namespace_create(ioctx.raw(), c_ns.as_ptr()) };
    check_rc(rc, format!("rbd_namespace_create({namespace})"))
}

pub fn namespace_exists(ioctx: &RadosIoCtx, namespace: &str) -> Result<bool, CephalopodError> {
    let c_ns = CString::new(namespace)?;
    let mut exists = false;
    let rc = unsafe { rbd::rbd_namespace_exists(ioctx.raw(), c_ns.as_ptr(), &mut exists) };
    check_rc(rc, format!("rbd_namespace_exists({namespace})"))?;
    Ok(exists)
}

pub fn namespace_list(ioctx: &RadosIoCtx) -> Result<Vec<String>, CephalopodError> {
    let mut size: usize = 4096;

    loop {
        let mut buf: Vec<u8> = vec![0u8; size];
        let rc =
            unsafe { rbd::rbd_namespace_list(ioctx.raw(), buf.as_mut_ptr() as *mut _, &mut size) };

        if rc == -34 {
            // ERANGE — size has been updated, retry
            continue;
        }
        check_rc(rc, "rbd_namespace_list")?;

        if size == 0 {
            return Ok(vec![]);
        }

        // Buffer is NUL-separated, double-NUL terminated
        let names = buf[..size]
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        return Ok(names);
    }
}

// ---------------------------------------------------------------------------
// Image CRUD (take ioctx)
// ---------------------------------------------------------------------------

/// List all images in the pool (current namespace of ioctx).
pub fn image_list(ioctx: &RadosIoCtx) -> Result<Vec<String>, CephalopodError> {
    let mut max_images: usize = 64;

    loop {
        let mut images: Vec<rbd::rbd_image_spec_t> = vec![
            rbd::rbd_image_spec_t {
                id: ptr::null_mut(),
                name: ptr::null_mut(),
            };
            max_images
        ];

        let rc = unsafe { rbd::rbd_list2(ioctx.raw(), images.as_mut_ptr(), &mut max_images) };

        if rc == -34 {
            // ERANGE — grow and retry
            max_images *= 2;
            continue;
        }
        check_rc(rc, "rbd_list2")?;

        let result: Vec<String> = images[..max_images]
            .iter()
            .map(|img| {
                if img.name.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(img.name) }
                        .to_string_lossy()
                        .into_owned()
                }
            })
            .filter(|s| !s.is_empty())
            .collect();

        unsafe { rbd::rbd_image_spec_list_cleanup(images.as_mut_ptr(), max_images) };
        return Ok(result);
    }
}

/// Create an image with the given size in MiB. Uses format 2.
pub fn image_create(ioctx: &RadosIoCtx, name: &str, size_mib: u32) -> Result<(), CephalopodError> {
    let c_name = CString::new(name)?;
    let size_bytes = (size_mib as u64) * 1024 * 1024;

    let mut opts = ImageOptions::new();
    opts.set_u64(rbd::RBD_IMAGE_OPTION_FORMAT, 2)?;

    let rc = unsafe { rbd::rbd_create4(ioctx.raw(), c_name.as_ptr(), size_bytes, opts.raw()) };
    check_rc(rc, format!("rbd_create4({name}, {size_mib}MiB)"))
}

/// Remove an image. Image must have no snapshots.
pub fn image_remove(ioctx: &RadosIoCtx, name: &str) -> Result<(), CephalopodError> {
    let c_name = CString::new(name)?;
    let rc = unsafe { rbd::rbd_remove(ioctx.raw(), c_name.as_ptr()) };
    check_rc(rc, format!("rbd_remove({name})"))
}

/// Check if an image exists by trying to open it.
pub fn image_exists(ioctx: &RadosIoCtx, name: &str) -> Result<bool, CephalopodError> {
    match RbdImage::open(ioctx, name) {
        Ok(_) => Ok(true),
        Err(CephalopodError::NotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Get image stat info.
pub fn image_stat(ioctx: &RadosIoCtx, name: &str) -> Result<ImageInfo, CephalopodError> {
    let img = RbdImage::open(ioctx, name)?;
    let mut info: rbd::rbd_image_info_t = unsafe { mem::zeroed() };
    let rc = unsafe {
        rbd::rbd_stat(
            img.raw(),
            &mut info,
            mem::size_of::<rbd::rbd_image_info_t>(),
        )
    };
    check_rc(rc, format!("rbd_stat({name})"))?;

    let prefix = unsafe { CStr::from_ptr(info.block_name_prefix.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let parent = unsafe { CStr::from_ptr(info.parent_name.as_ptr()) }
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
}

/// Get image size in bytes.
pub fn image_get_size(ioctx: &RadosIoCtx, name: &str) -> Result<u64, CephalopodError> {
    let img = RbdImage::open(ioctx, name)?;
    let mut size: u64 = 0;
    let rc = unsafe { rbd::rbd_get_size(img.raw(), &mut size) };
    check_rc(rc, format!("rbd_get_size({name})"))?;
    Ok(size)
}

/// Resize (grow only) an image to the given size in MiB.
pub fn image_resize(ioctx: &RadosIoCtx, name: &str, size_mib: u32) -> Result<(), CephalopodError> {
    let img = RbdImage::open(ioctx, name)?;
    let size_bytes = (size_mib as u64) * 1024 * 1024;
    let rc = unsafe { rbd::rbd_resize2(img.raw(), size_bytes, false, None, ptr::null_mut()) };
    check_rc(rc, format!("rbd_resize2({name}, {size_mib}MiB)"))
}

/// List watchers on an image.
pub fn image_watchers(ioctx: &RadosIoCtx, name: &str) -> Result<Vec<WatcherInfo>, CephalopodError> {
    let img = RbdImage::open(ioctx, name)?;

    let mut max_watchers: usize = 16;
    loop {
        let mut watchers: Vec<rbd::rbd_image_watcher_t> = vec![
            rbd::rbd_image_watcher_t {
                addr: ptr::null_mut(),
                id: 0,
                cookie: 0,
            };
            max_watchers
        ];

        let rc =
            unsafe { rbd::rbd_watchers_list(img.raw(), watchers.as_mut_ptr(), &mut max_watchers) };

        if rc == -34 {
            // ERANGE
            max_watchers *= 2;
            continue;
        }
        check_rc(rc, format!("rbd_watchers_list({name})"))?;

        let result: Vec<WatcherInfo> = watchers[..max_watchers]
            .iter()
            .map(|w| WatcherInfo {
                addr: if w.addr.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(w.addr) }
                        .to_string_lossy()
                        .into_owned()
                },
                id: w.id,
                cookie: w.cookie,
            })
            .collect();

        unsafe { rbd::rbd_watchers_list_cleanup(watchers.as_mut_ptr(), max_watchers) };
        return Ok(result);
    }
}

// ---------------------------------------------------------------------------
// Snapshot operations
// ---------------------------------------------------------------------------

/// List snapshots on an image.
pub fn snap_list(ioctx: &RadosIoCtx, image_name: &str) -> Result<Vec<SnapInfo>, CephalopodError> {
    let img = RbdImage::open(ioctx, image_name)?;

    let mut max_snaps: c_int = 64;
    loop {
        let mut snaps: Vec<rbd::rbd_snap_info_t> = vec![
            rbd::rbd_snap_info_t {
                id: 0,
                size: 0,
                name: ptr::null(),
            };
            max_snaps as usize
        ];

        let rc = unsafe { rbd::rbd_snap_list(img.raw(), snaps.as_mut_ptr(), &mut max_snaps) };

        if rc == -34 {
            // ERANGE
            max_snaps *= 2;
            continue;
        }
        if rc < 0 {
            check_rc(rc, format!("rbd_snap_list({image_name})"))?;
        }

        let count = rc as usize; // on success, rc = number of snaps
        let result: Vec<SnapInfo> = snaps[..count]
            .iter()
            .map(|s| SnapInfo {
                id: s.id,
                size: s.size,
                name: if s.name.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(s.name) }
                        .to_string_lossy()
                        .into_owned()
                },
            })
            .collect();

        unsafe { rbd::rbd_snap_list_end(snaps.as_mut_ptr()) };
        return Ok(result);
    }
}

/// Create a snapshot.
pub fn snap_create(
    ioctx: &RadosIoCtx,
    image_name: &str,
    snap_name: &str,
) -> Result<(), CephalopodError> {
    let img = RbdImage::open(ioctx, image_name)?;
    let c_snap = CString::new(snap_name)?;
    let rc = unsafe { rbd::rbd_snap_create(img.raw(), c_snap.as_ptr()) };
    check_rc(rc, format!("rbd_snap_create({image_name}@{snap_name})"))
}

/// Remove a snapshot. Must not be protected.
pub fn snap_remove(
    ioctx: &RadosIoCtx,
    image_name: &str,
    snap_name: &str,
) -> Result<(), CephalopodError> {
    let img = RbdImage::open(ioctx, image_name)?;
    let c_snap = CString::new(snap_name)?;
    let rc = unsafe { rbd::rbd_snap_remove(img.raw(), c_snap.as_ptr()) };
    check_rc(rc, format!("rbd_snap_remove({image_name}@{snap_name})"))
}

/// Protect a snapshot (required before cloning).
pub fn snap_protect(
    ioctx: &RadosIoCtx,
    image_name: &str,
    snap_name: &str,
) -> Result<(), CephalopodError> {
    let img = RbdImage::open(ioctx, image_name)?;
    let c_snap = CString::new(snap_name)?;
    let rc = unsafe { rbd::rbd_snap_protect(img.raw(), c_snap.as_ptr()) };
    check_rc(rc, format!("rbd_snap_protect({image_name}@{snap_name})"))
}

/// Unprotect a snapshot. Must have no clone children.
pub fn snap_unprotect(
    ioctx: &RadosIoCtx,
    image_name: &str,
    snap_name: &str,
) -> Result<(), CephalopodError> {
    let img = RbdImage::open(ioctx, image_name)?;
    let c_snap = CString::new(snap_name)?;
    let rc = unsafe { rbd::rbd_snap_unprotect(img.raw(), c_snap.as_ptr()) };
    check_rc(rc, format!("rbd_snap_unprotect({image_name}@{snap_name})"))
}

/// Check if a snapshot is protected.
pub fn snap_is_protected(
    ioctx: &RadosIoCtx,
    image_name: &str,
    snap_name: &str,
) -> Result<bool, CephalopodError> {
    let img = RbdImage::open(ioctx, image_name)?;
    let c_snap = CString::new(snap_name)?;
    let mut is_protected: c_int = 0;
    let rc = unsafe { rbd::rbd_snap_is_protected(img.raw(), c_snap.as_ptr(), &mut is_protected) };
    check_rc(
        rc,
        format!("rbd_snap_is_protected({image_name}@{snap_name})"),
    )?;
    Ok(is_protected != 0)
}

/// Purge all snapshots on an image. Unprotects any protected snaps first.
pub fn snap_purge(ioctx: &RadosIoCtx, image_name: &str) -> Result<(), CephalopodError> {
    let snaps = snap_list(ioctx, image_name)?;
    for snap in &snaps {
        // Try to unprotect — ignore errors if it's already unprotected
        if snap_is_protected(ioctx, image_name, &snap.name).unwrap_or(false) {
            snap_unprotect(ioctx, image_name, &snap.name)?;
        }
        snap_remove(ioctx, image_name, &snap.name)?;
    }
    Ok(())
}

/// Clone a protected snapshot to a new image. Uses clone format 2 for cross-namespace support.
///
/// `src_ioctx` is the ioctx (with namespace set) for the parent image.
/// `dst_ioctx` is the ioctx (with namespace set) for the child image.
/// These may be the same ioctx if source and destination are in the same namespace.
pub fn snap_clone(
    src_ioctx: &RadosIoCtx,
    parent_image: &str,
    parent_snap: &str,
    dst_ioctx: &RadosIoCtx,
    child_image: &str,
) -> Result<(), CephalopodError> {
    let c_parent = CString::new(parent_image)?;
    let c_snap = CString::new(parent_snap)?;
    let c_child = CString::new(child_image)?;

    let mut opts = ImageOptions::new();
    opts.set_u64(rbd::RBD_IMAGE_OPTION_CLONE_FORMAT, 2)?;

    let rc = unsafe {
        rbd::rbd_clone3(
            src_ioctx.raw(),
            c_parent.as_ptr(),
            c_snap.as_ptr(),
            dst_ioctx.raw(),
            c_child.as_ptr(),
            opts.raw(),
        )
    };
    check_rc(
        rc,
        format!("rbd_clone3({parent_image}@{parent_snap} -> {child_image})"),
    )
}

/// List children (clones) of a snapshot.
pub fn snap_list_children(
    ioctx: &RadosIoCtx,
    image_name: &str,
    snap_name: &str,
) -> Result<Vec<ChildInfo>, CephalopodError> {
    let img = RbdImage::open_at_snap(ioctx, image_name, snap_name)?;

    let mut max_images: usize = 16;
    loop {
        let mut images: Vec<rbd::rbd_linked_image_spec_t> = vec![
            rbd::rbd_linked_image_spec_t {
                pool_id: 0,
                pool_name: ptr::null_mut(),
                pool_namespace: ptr::null_mut(),
                image_id: ptr::null_mut(),
                image_name: ptr::null_mut(),
                trash: false,
            };
            max_images
        ];

        let rc =
            unsafe { rbd::rbd_list_children3(img.raw(), images.as_mut_ptr(), &mut max_images) };

        if rc == -34 {
            // ERANGE
            max_images *= 2;
            continue;
        }
        check_rc(rc, format!("rbd_list_children3({image_name}@{snap_name})"))?;

        let result: Vec<ChildInfo> = images[..max_images]
            .iter()
            .map(|c| {
                let str_field = |p: *mut std::os::raw::c_char| -> String {
                    if p.is_null() {
                        String::new()
                    } else {
                        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
                    }
                };
                ChildInfo {
                    pool_id: c.pool_id,
                    pool_name: str_field(c.pool_name),
                    pool_namespace: str_field(c.pool_namespace),
                    image_id: str_field(c.image_id),
                    image_name: str_field(c.image_name),
                    trash: c.trash,
                }
            })
            .collect();

        unsafe {
            rbd::rbd_linked_image_spec_list_cleanup(images.as_mut_ptr(), max_images);
        }
        return Ok(result);
    }
}
