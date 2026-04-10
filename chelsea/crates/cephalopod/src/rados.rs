//! Safe wrappers around librados: cluster handle and I/O context.

use std::ffi::{CStr, CString};
use std::ptr;

use crate::error::{CephalopodError, check_rc};
use crate::ffi::rados;

/// A connection to a Ceph cluster. Owns the `rados_t` handle.
///
/// **Thread safety**: librados documents `rados_t` as thread-safe after `rados_connect`.
/// This type is `Send + Sync`.
#[derive(Debug)]
pub struct RadosCluster {
    handle: rados::rados_t,
    id: String,
}

// SAFETY: librados rados_t is thread-safe after connect.
unsafe impl Send for RadosCluster {}
unsafe impl Sync for RadosCluster {}

impl RadosCluster {
    /// Connect to the Ceph cluster as the given client id (e.g. "chelsea").
    /// Reads the default ceph.conf and the keyring for `client.{id}`.
    pub fn connect(id: &str) -> Result<Self, CephalopodError> {
        let c_id = CString::new(id)?;
        let mut handle: rados::rados_t = ptr::null_mut();

        // Create the cluster handle
        let rc = unsafe { rados::rados_create(&mut handle, c_id.as_ptr()) };
        check_rc(rc, "rados_create")?;

        // Read default config
        let rc = unsafe { rados::rados_conf_read_file(handle, ptr::null()) };
        if rc < 0 {
            unsafe { rados::rados_shutdown(handle) };
            return Err(CephalopodError::from_errno(rc, "rados_conf_read_file"));
        }

        // Connect
        let rc = unsafe { rados::rados_connect(handle) };
        if rc < 0 {
            unsafe { rados::rados_shutdown(handle) };
            return Err(CephalopodError::from_errno(rc, "rados_connect"));
        }

        Ok(Self {
            handle,
            id: id.to_string(),
        })
    }

    /// Get a config value from the cluster (e.g. "mon_host").
    pub fn conf_get(&self, key: &str) -> Result<String, CephalopodError> {
        let c_key = CString::new(key)?;
        let mut buf = vec![0u8; 4096];
        let rc = unsafe {
            rados::rados_conf_get(
                self.handle,
                c_key.as_ptr(),
                buf.as_mut_ptr() as *mut std::os::raw::c_char,
                buf.len(),
            )
        };
        check_rc(rc, format!("rados_conf_get({key})"))?;
        let value = unsafe { CStr::from_ptr(buf.as_ptr() as *const std::os::raw::c_char) };
        Ok(value.to_string_lossy().into_owned())
    }

    /// Get the client id (e.g. "chelsea").
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Create an I/O context for the given pool.
    pub fn ioctx(&self, pool_name: &str) -> Result<RadosIoCtx, CephalopodError> {
        let c_pool = CString::new(pool_name)?;
        let mut ioctx: rados::rados_ioctx_t = ptr::null_mut();

        let rc = unsafe { rados::rados_ioctx_create(self.handle, c_pool.as_ptr(), &mut ioctx) };
        check_rc(rc, format!("rados_ioctx_create(pool={pool_name})"))?;

        Ok(RadosIoCtx { handle: ioctx })
    }
}

impl Drop for RadosCluster {
    fn drop(&mut self) {
        unsafe { rados::rados_shutdown(self.handle) };
    }
}

/// An I/O context bound to a specific pool. Owns the `rados_ioctx_t` handle.
///
/// **Thread safety**: Each ioctx should only be used from one thread at a time
/// (or protected by a mutex). We mark it `Send` so it can be moved between
/// tasks, but not `Sync`.
pub struct RadosIoCtx {
    handle: rados::rados_ioctx_t,
}

// SAFETY: rados_ioctx_t can be sent between threads.
unsafe impl Send for RadosIoCtx {}

impl RadosIoCtx {
    /// Get the raw handle. Used by librbd functions that take an ioctx.
    pub(crate) fn raw(&self) -> rados::rados_ioctx_t {
        self.handle
    }

    /// Set the namespace for subsequent operations on this ioctx.
    /// Pass an empty string to reset to the default namespace.
    pub fn set_namespace(&self, namespace: &str) -> Result<(), CephalopodError> {
        let c_ns = CString::new(namespace)?;
        unsafe { rados::rados_ioctx_set_namespace(self.handle, c_ns.as_ptr()) };
        Ok(())
    }
}

impl Drop for RadosIoCtx {
    fn drop(&mut self) {
        unsafe { rados::rados_ioctx_destroy(self.handle) };
    }
}
