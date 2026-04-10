use std::{
    fs::remove_file,
    ops::Deref,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::{
    docker::{docker_build_temp, docker_create_temp, docker_export},
    error::Error,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct DockerDump {
    /// Path to the image file
    pub path: PathBuf,
}

impl DockerDump {
    pub async fn new(dockerfile_path: &Path, output_file: PathBuf) -> Result<Self, Error> {
        let output_file = output_file.with_extension("tar");
        debug!(
            ?dockerfile_path,
            ?output_file,
            "Creating Docker dump... (this may take a while)"
        );
        // # Build image and create exportable container
        let temp_image = docker_build_temp(dockerfile_path).await?;
        let temp_container = docker_create_temp(&temp_image.image_id).await?;

        // Export the file system to a tarball
        let tarball_path = docker_export(&temp_container.container_id, &output_file).await?;

        info!(
            path = tarball_path.display().to_string(),
            "Successfully created Docker dump"
        );
        Ok(Self { path: tarball_path })
    }

    pub async fn from_existing(tarball_path: PathBuf) -> Result<Self, Error> {
        match tokio::fs::try_exists(&tarball_path).await {
            Ok(true) => Ok(Self { path: tarball_path }),
            _ => Err(Error::IoError(
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file {} not found", tarball_path.display().to_string()),
                )
                .to_string(),
            )),
        }
    }

    /// Extract the contents of the tarball to the given directory
    pub async fn extract(&self, output_dir: &Path) -> Result<(), Error> {
        debug!(?output_dir, "Extracting Docker dump");

        let output = Command::new("tar")
            .arg("xf")
            .arg(&self.path)
            .arg("-C")
            .arg(output_dir.display().to_string())
            .output()
            .await
            .map_err(|e| Error::IoError(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr).map_err(|e| Error::Tar(e.to_string()))?;
            return Err(Error::Tar(stderr));
        }

        info!(?output_dir, "Successfully extracted Docker dump");
        Ok(())
    }
}

pub struct TempDockerDump {
    docker_dump: DockerDump,
}

impl TempDockerDump {
    pub async fn new(dockerfile_path: &Path, output_file: PathBuf) -> Result<Self, Error> {
        let docker_dump = DockerDump::new(dockerfile_path, output_file).await?;
        Ok(Self { docker_dump })
    }
}

impl Drop for TempDockerDump {
    fn drop(&mut self) {
        let path_str = self
            .docker_dump
            .path
            .to_str()
            .unwrap_or("(failed to convert path to str)");
        if let Err(e) = remove_file(&self.docker_dump.path) {
            warn!("Error cleaning up image file at {path_str}: {e}");
        }
        info!(path = path_str, "Successfully removed Docker dump");
    }
}

impl Deref for TempDockerDump {
    type Target = DockerDump;
    fn deref(&self) -> &Self::Target {
        &self.docker_dump
    }
}
