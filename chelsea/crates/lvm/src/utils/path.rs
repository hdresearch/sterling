use crate::error::LvmError;
use std::path::PathBuf;

/// Gets the data directory for the app, creating it if it doesn't exist.
pub fn data_dir() -> Result<PathBuf, LvmError> {
    let path = dirs::home_dir()
        .expect("Could not find home directory")
        .join(".local")
        .join("share")
        .join("chelsea-manager");

    if !path.exists() {
        std::fs::create_dir_all(&path)?;
    }

    Ok(path)
}
