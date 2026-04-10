use tracing::{debug, warn};

/// A Docker image that automatically removes itself when dropped.
pub struct TempDockerImageGuard {
    pub image_id: String,
}

impl TempDockerImageGuard {
    pub fn new(image_id: String) -> Self {
        debug!(%image_id, "Creating temporary docker image guard");
        Self { image_id }
    }
}

impl Drop for TempDockerImageGuard {
    fn drop(&mut self) {
        debug!(%self.image_id, "Removing docker image");
        let output = std::process::Command::new("docker")
            .arg("rmi")
            .arg(&self.image_id)
            .output();

        if let Err(e) = output {
            warn!("Error removing docker image {}: {}", self.image_id, e);
        } else {
            debug!(%self.image_id, "Successfully removed docker image");
        }
    }
}

/// A Docker container that automatically removes itself when dropped.
pub struct TempDockerContainerGuard {
    pub container_id: String,
}

impl TempDockerContainerGuard {
    pub fn new(container_id: String) -> Self {
        debug!(%container_id, "Creating temporary docker container guard");
        Self { container_id }
    }
}

impl Drop for TempDockerContainerGuard {
    fn drop(&mut self) {
        debug!(%self.container_id, "Removing docker container");
        let output = std::process::Command::new("docker")
            .arg("rm")
            .arg(&self.container_id)
            .output();

        if let Err(e) = output {
            warn!(
                "Error removing docker container {}: {}",
                self.container_id, e
            );
        } else {
            debug!(%self.container_id, "Successfully removed docker container");
        }
    }
}
