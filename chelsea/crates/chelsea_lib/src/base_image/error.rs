use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BaseImageError {
    #[error("Failed to configure filesystem: {0}")]
    ConfigureFilesystem(String),

    #[error("Failed to create directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to write file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to create symlink from {from} to {to}: {source}")]
    CreateSymlink {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to set file permissions on {path}: {source}")]
    SetPermissions {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to download from S3: {0}")]
    S3Download(String),

    #[error("Failed to create Docker dump: {0}")]
    DockerDump(String),

    #[error("Failed to extract tarball: {0}")]
    ExtractTarball(String),

    #[error("RBD client error: {0}")]
    RbdClient(#[from] ceph::RbdClientError),

    #[error("Failed to format filesystem: {0}")]
    FormatFilesystem(String),

    #[error("Failed to mount device: {0}")]
    MountDevice(String),

    #[error("Failed to unmount device: {0}")]
    UnmountDevice(String),

    #[error("Failed to copy files: {0}")]
    CopyFiles(String),

    #[error("Image already exists: {0}")]
    ImageAlreadyExists(String),

    #[error("Image not found: {0}")]
    ImageNotFound(String),

    #[error("Image '{image_name}' is in use by VMs: {vm_ids:?}")]
    ImageInUse {
        image_name: String,
        vm_ids: Vec<String>,
    },

    #[error("Image '{0}' has child clones and cannot be deleted")]
    ImageHasChildClones(String),

    #[error(
        "Invalid image name '{0}': must contain only alphanumeric characters, hyphens, underscores, and periods"
    )]
    InvalidImageName(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_not_found_error_display() {
        let err = BaseImageError::ImageNotFound("my-image".to_string());
        assert_eq!(err.to_string(), "Image not found: my-image");
    }

    #[test]
    fn test_image_already_exists_error_display() {
        let err = BaseImageError::ImageAlreadyExists("existing-image".to_string());
        assert_eq!(err.to_string(), "Image already exists: existing-image");
    }

    #[test]
    fn test_image_in_use_error_display() {
        let err = BaseImageError::ImageInUse {
            image_name: "busy-image".to_string(),
            vm_ids: vec!["vm-1".to_string(), "vm-2".to_string()],
        };
        let msg = err.to_string();
        assert!(msg.contains("busy-image"));
        assert!(msg.contains("in use by VMs"));
        assert!(msg.contains("vm-1"));
        assert!(msg.contains("vm-2"));
    }

    #[test]
    fn test_image_has_child_clones_error_display() {
        let err = BaseImageError::ImageHasChildClones("parent-image".to_string());
        assert_eq!(
            err.to_string(),
            "Image 'parent-image' has child clones and cannot be deleted"
        );
    }

    #[test]
    fn test_invalid_image_name_error_display() {
        let err = BaseImageError::InvalidImageName("bad;name".to_string());
        let msg = err.to_string();
        assert!(msg.contains("bad;name"));
        assert!(msg.contains("Invalid image name"));
    }

    #[test]
    fn test_error_debug_format() {
        let err = BaseImageError::ImageNotFound("test".to_string());
        let debug = format!("{:?}", err);
        assert!(debug.contains("ImageNotFound"));

        let err = BaseImageError::ImageHasChildClones("test".to_string());
        let debug = format!("{:?}", err);
        assert!(debug.contains("ImageHasChildClones"));
    }
}
