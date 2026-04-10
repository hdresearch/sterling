use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::debug;

use super::types::TempDockerContainerGuard;
use crate::{error::Error, types::TempDockerImageGuard};

/// Builds the given dockerfile, returning the image ID on success.
pub async fn docker_build(dockerfile: &Path) -> Result<String, Error> {
    debug!(?dockerfile, "Building docker image from dockerfile");

    let context_dir = dockerfile.parent().ok_or(Error::DockerBuild(format!(
        "Unable to determine parent directory for dockerfile at {dockerfile:?}"
    )))?;

    let output = Command::new("docker")
        .arg("build")
        .arg("--network=host")
        .arg("-q")
        .arg("-f")
        .arg(dockerfile.display().to_string())
        .arg(context_dir.display().to_string())
        .output()
        .await
        .map_err(|e| Error::DockerBuild(e.to_string()))?;

    if !output.status.success() {
        let stderr =
            String::from_utf8(output.stderr).map_err(|e| Error::DockerBuild(e.to_string()))?;
        return Err(Error::DockerBuild(stderr));
    }

    let image_id = String::from_utf8(output.stdout)
        .map_err(|e| Error::DockerBuild(e.to_string()))?
        .trim()
        .to_string();

    debug!(%image_id, "Successfully built docker image");
    Ok(image_id)
}

/// Builds a docker image and returns a guard that will automatically remove it when dropped.
pub async fn docker_build_temp(dockerfile: &Path) -> Result<TempDockerImageGuard, Error> {
    let image_id = docker_build(dockerfile).await?;
    debug!(%image_id, "Created temporary docker image");
    Ok(TempDockerImageGuard::new(image_id))
}

/// Creates a container from the given image ID, returning the container ID on success.
pub async fn docker_create(image_id: &str) -> Result<String, Error> {
    debug!(%image_id, "Creating container from image");

    let output = Command::new("docker")
        .arg("create")
        .arg(image_id)
        .output()
        .await
        .map_err(|e| Error::DockerCreate(e.to_string()))?;

    if !output.status.success() {
        let stderr =
            String::from_utf8(output.stderr).map_err(|e| Error::DockerCreate(e.to_string()))?;
        return Err(Error::DockerCreate(stderr));
    }

    let container_id = String::from_utf8(output.stdout)
        .map_err(|e| Error::DockerCreate(e.to_string()))?
        .trim()
        .to_string();

    debug!(%container_id, "Successfully created container");
    Ok(container_id)
}

/// Creates a container from the given image ID and returns a guard that will automatically remove it when dropped.
pub async fn docker_create_temp(image_id: &str) -> Result<TempDockerContainerGuard, Error> {
    let container_id = docker_create(image_id).await?;
    debug!(%container_id, "Created temporary container");
    Ok(TempDockerContainerGuard::new(container_id))
}

/// Exports a docker container's filesystem to a tarball, returning the path to the tarball.
pub async fn docker_export(container_id: &str, tarball_path: &Path) -> Result<PathBuf, Error> {
    debug!(%container_id, "Exporting container filesystem");

    let output = Command::new("docker")
        .arg("export")
        .arg(container_id)
        .arg("-o")
        .arg(tarball_path.display().to_string())
        .output()
        .await
        .map_err(|e| Error::DockerExport(e.to_string()))?;

    if !output.status.success() {
        let stderr =
            String::from_utf8(output.stderr).map_err(|e| Error::DockerExport(e.to_string()))?;
        return Err(Error::DockerExport(stderr));
    }

    debug!(?tarball_path, "Successfully exported container filesystem");
    Ok(PathBuf::from(tarball_path))
}
