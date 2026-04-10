use serde::Deserialize;

/// Info for a single RBD image, as returned from `rbd info --format json`
#[derive(Debug, Clone, Deserialize)]
pub struct RbdImageInfo {
    pub name: String,
    pub id: String,
    /// The image size, in bytes. For MiB, see size_mib()
    pub size: u64,
    pub objects: u64,
    pub order: u64,
    pub object_size: u64,
    pub snapshot_count: u64,
    pub block_name_prefix: String,
    pub format: u64,
    pub features: Vec<String>,
    pub op_features: Vec<String>,
    pub flags: Vec<String>,
    // Unclear if these are UTC or what, so leaving them as strings until they become relevant.
    pub create_timestamp: String,
    pub access_timestamp: String,
    pub modify_timestamp: String,

    pub parent: Option<RbdImageParent>,
}

impl RbdImageInfo {
    /// The image size, in mebibytes.
    pub fn size_mib(&self) -> u32 {
        (self.size / (1024 * 1024)) as u32
    }
}

/// Status for a single RBD image, as returned from `rbd status --format json`
#[derive(Debug, Clone, Deserialize)]
pub struct RbdImageStatus {
    pub watchers: Vec<RbdImageWatcher>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RbdImageWatcher {
    pub address: String,
    pub client: u64,
    pub cookie: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RbdImageParent {
    pub pool: String,
    pub pool_namespace: String,
    pub image: String,
    pub id: String,
    pub snapshot: String,
    pub trash: bool,
    pub overlap: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RbdSnapshotInfo {
    pub id: u64,
    pub name: String,
    pub size: u64,
    #[serde(deserialize_with = "deserialize_bool_from_string")]
    pub protected: bool,
    pub timestamp: String,
}

// Helper function to deserialize "protected": "true"/"false" as bool
fn deserialize_bool_from_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "invalid boolean string: {}",
            other
        ))),
    }
}
