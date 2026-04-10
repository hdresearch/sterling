use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreateVmVolumeFromImageError {
    #[error("Failed to create VmVolume from non-existent image '{0}'")]
    ImageNotFound(String),
    #[error("Internal error while creating VmVolume from image: {0}")]
    Other(String),
}
