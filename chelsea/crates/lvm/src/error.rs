use std::path::PathBuf;

#[derive(Debug)]
pub enum LvmError {
    BackingFileCreate(String),
    BackingFileDelete(String),
    BackingFileFromExisting(String),
    LoopDeviceCreate(String),
    LoopDeviceDelete(String),
    LoopDeviceFromExisting(String),
    PhysicalVolumeCreate(String),
    PhysicalVolumeDelete(String),
    PhysicalVolumeFromExisting(String),
    VolumeGroupCreate(String),
    VolumeGroupDelete(Vec<String>),
    VolumeGroupFromExisting(String),
    VolumeGroupEmpty,
    ThinPoolCreate(String),
    ThinPoolDelete(String),
    ThinPoolFromExisting(String),
    ThinVolumeCreate(String),
    ThinVolumeMkfs(String),
    ThinVolumeDelete(String),
    ThinVolumeArchive(String),
    ThinVolumeFromExisting(String),
    ThinVolumeParentUpgrade(String),
    IoError(std::io::Error),
    PathContainsInvalidUtf8(PathBuf),
}

impl std::error::Error for LvmError {}

impl std::fmt::Display for LvmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BackingFileCreate(e) => write!(f, "Error creating backing file: {e}"),
            Self::BackingFileDelete(e) => write!(f, "Error deleting backing file: {e}"),
            Self::BackingFileFromExisting(e) => {
                write!(f, "Error creating backing file from existing file: {e}")
            }
            Self::LoopDeviceCreate(e) => write!(f, "Error creating loop device: {e}"),
            Self::LoopDeviceDelete(e) => write!(f, "Error deleting loop device: {e}"),
            Self::LoopDeviceFromExisting(e) => {
                write!(f, "Error creating loop device from existing file: {e}")
            }
            Self::PhysicalVolumeCreate(e) => write!(f, "Error creating physical volume: {e}"),
            Self::PhysicalVolumeDelete(e) => write!(f, "Error deleting physical volume: {e}"),
            Self::PhysicalVolumeFromExisting(e) => write!(
                f,
                "Error creating physical volume from existing device: {e}"
            ),
            Self::VolumeGroupCreate(e) => write!(f, "Error creating volume group: {e}"),
            Self::VolumeGroupDelete(e) => {
                write!(f, "Errors deleting volume group: {}", e.join("; "))
            }
            Self::VolumeGroupFromExisting(e) => {
                write!(f, "Error creating volume group from existing volumes: {e}")
            }
            Self::VolumeGroupEmpty => write!(f, "Volume group unexpectedly empty"),
            Self::ThinPoolCreate(e) => write!(f, "Error creating thin pool: {e}"),
            Self::ThinPoolDelete(e) => write!(f, "Error deleting thin pool: {e}"),
            Self::ThinPoolFromExisting(e) => write!(
                f,
                "Error creating thin pool from existing volume group: {e}"
            ),
            Self::ThinVolumeCreate(e) => write!(f, "Error creating thin volume: {e}"),
            Self::ThinVolumeMkfs(e) => write!(f, "Error making filesystem on thin volume: {e}"),
            Self::ThinVolumeDelete(e) => write!(f, "Error deleting thin volume: {e}"),
            Self::ThinVolumeArchive(e) => write!(f, "Error archiving thin volume: {e}"),
            Self::ThinVolumeFromExisting(e) => {
                write!(f, "Error creating thin volume from existing thin pool: {e}")
            }
            Self::ThinVolumeParentUpgrade(id) => {
                write!(f, "Failed to upgrade parent weak reference for volume {id}")
            }
            Self::IoError(e) => write!(f, "IO error: {e}"),
            Self::PathContainsInvalidUtf8(path) => write!(
                f,
                "Path contains invalid UTF-8: {}",
                path.display().to_string()
            ),
        }
    }
}

impl From<std::io::Error> for LvmError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}
