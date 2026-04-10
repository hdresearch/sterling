use std::{io, os::unix::fs::chown, path::Path};

use thiserror::Error;
use util::linux::{UserError, UserInfo, get_or_create_system_user};
use vers_config::VersConfig;

#[derive(Debug, Error)]
pub enum ChownVmError {
    #[error("user error: {0}")]
    User(#[from] UserError),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// Execute chown using the default VM user (defined by VM_USER_NAME)
pub fn chown_vm<P: AsRef<Path>>(dir: P) -> Result<(), ChownVmError> {
    let user_info = get_or_create_vm_user()?;
    let (uid, gid) = (Some(user_info.uid), Some(user_info.gid));

    chown(dir, uid, gid)?;
    Ok(())
}

/// Get the UID and GID for the VM user (defined by VM_USER_NAME)
pub fn get_or_create_vm_user() -> Result<UserInfo, UserError> {
    get_or_create_system_user(&VersConfig::chelsea().vm_user_name)
}
