use std::ffi::NulError;

use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum CephalopodError {
    /// A librados or librbd function returned a negative errno.
    #[error("{context}: {message} (errno {errno})")]
    Ceph {
        errno: i32,
        message: String,
        context: String,
    },

    /// The requested resource was not found (ENOENT).
    #[error("not found: {0}")]
    NotFound(String),

    /// The resource already exists (EEXIST).
    #[error("already exists: {0}")]
    AlreadyExists(String),

    /// A string argument contained an interior NUL byte.
    #[error("invalid argument (interior NUL): {0}")]
    NulByte(#[from] NulError),

    /// A device map/unmap operation failed (still uses exec).
    #[error("device operation failed: {0}")]
    Device(String),
}

impl CephalopodError {
    /// Create from an errno returned by a Ceph C function.
    /// Negative errnos are converted to positive for the message lookup.
    pub(crate) fn from_errno(errno: i32, context: impl Into<String>) -> Self {
        let abs = errno.unsigned_abs() as i32;
        // ENOENT = 2
        if abs == 2 {
            return Self::NotFound(context.into());
        }
        // EEXIST = 17
        if abs == 17 {
            return Self::AlreadyExists(context.into());
        }
        let message = std::io::Error::from_raw_os_error(abs).to_string();
        Self::Ceph {
            errno: abs,
            message,
            context: context.into(),
        }
    }
}

/// Check a return code from a Ceph C function. Returns Ok(()) on success (rc >= 0),
/// or the appropriate CephalopodError on failure.
pub(crate) fn check_rc(rc: i32, context: impl Into<String>) -> Result<(), CephalopodError> {
    if rc < 0 {
        Err(CephalopodError::from_errno(rc, context))
    } else {
        Ok(())
    }
}
