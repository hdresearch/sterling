use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub url: String,
    pub sha256_hash: String,
    pub downloaded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryMetadata {
    pub asset_id: u64,
    pub size: u64,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub path: String,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrecomputedChecksum {
    pub filename: String,
    pub size: u64,
    pub sha256: String,
}

/// GitHub Release Asset structure for parsing API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub browser_download_url: String,
}

/// GitHub Release structure for parsing API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub id: u64,
    pub tag_name: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub assets: Vec<GitHubAsset>,
}
