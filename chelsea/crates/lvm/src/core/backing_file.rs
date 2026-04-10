use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::LvmError;
use crate::path::data_dir;
use std::path::PathBuf;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackingFileCreateOptions {
    pub filename: String,
    pub size_mib: u32,
}

impl BackingFileCreateOptions {
    pub fn size_bytes(&self) -> u64 {
        self.size_mib as u64 * 1024 * 1024
    }
}

impl Default for BackingFileCreateOptions {
    /// Note that due to the way LayeredFsOptions are created, these defaults may differ from those of LayeredFsOptions::default()
    fn default() -> Self {
        Self {
            filename: "chelsea-manager.img".to_string(),
            size_mib: 2 * 1024,
        }
    }
}

/// A disk image, essentially
#[derive(Debug)]
pub struct BackingFile {
    path: PathBuf,
}

impl BackingFile {
    /// Creates a backing file in the data directory. Defaults to a file with 1024 1MB blocks.
    /// WARNING: This method uses `dd` - be sure the path is correct!
    pub async fn new(options: BackingFileCreateOptions) -> Result<Self, LvmError> {
        let data_path = data_dir()?;
        let path = data_path.join(&options.filename);

        let output = Command::new("fallocate")
            .args([
                "-l",
                &options.size_bytes().to_string(),
                &path.display().to_string(),
            ])
            .output()
            .await
            .map_err(|e| LvmError::BackingFileCreate(e.to_string()))?;

        if !output.status.success() {
            return Err(LvmError::BackingFileCreate(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        debug!(
            ?path,
            size_mib = %options.size_mib,
            "Backing file created",
        );

        Ok(Self { path })
    }

    pub fn from_existing(path: PathBuf) -> Result<Self, LvmError> {
        match path.exists() {
            true => Ok(Self { path }),
            false => Err(LvmError::BackingFileFromExisting(
                path.display().to_string(),
            )),
        }
    }

    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.path)
    }

    pub fn path_str(&self) -> String {
        self.path().display().to_string()
    }

    pub fn delete(&self) -> Result<(), LvmError> {
        let path = self.path_str();
        debug!(path = %path, "Deleting backing file");

        std::fs::remove_file(self.path()).map_err(|e| {
            debug!(path = %path, error = %e, "Failed to delete backing file");
            LvmError::BackingFileDelete(format!("{}: {}", path, e.to_string()))
        })?;

        debug!(path = %path, "Successfully deleted backing file");
        Ok(())
    }
}
