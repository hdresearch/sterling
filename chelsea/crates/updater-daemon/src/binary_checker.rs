use crate::{
    gh::get_github_token,
    types::{GitHubRelease, PrecomputedChecksum},
    UpdaterError,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

const REPO_RELEASES_URL: &str =
    "https://api.github.com/repos/hdresearch/chelsea/releases/tags/nightly";

pub async fn get_precomputed_checksum(client: Client) -> Result<PrecomputedChecksum, UpdaterError> {
    let token = get_github_token()
        .map_err(|e| UpdaterError::Authentication(format!("Failed to get GitHub token: {}", e)))?;

    // First get the release to find the JSON metadata asset
    let release_response = client
        .get(REPO_RELEASES_URL)
        .header("Authorization", format!("token {}", token))
        .header("User-Agent", "chelsea-updater")
        .send()
        .await?;

    if !release_response.status().is_success() {
        return Err(UpdaterError::GitHubApi(format!(
            "Failed to get release info: HTTP {} - {}",
            release_response.status(),
            release_response.text().await.unwrap_or_default()
        )));
    }

    let release: GitHubRelease = release_response.json().await?;

    // Find the JSON metadata file
    let json_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == "binary-linux.json")
        .ok_or_else(|| {
            UpdaterError::BinaryNotFound("binary-linux.json asset not found in release".to_string())
        })?;

    // Download the tiny JSON file (much faster than the binary!)
    let asset_url = format!(
        "https://api.github.com/repos/hdresearch/chelsea/releases/assets/{}",
        json_asset.id
    );
    let response = client
        .get(&asset_url)
        .header("Authorization", format!("token {}", token))
        .header("Accept", "application/octet-stream")
        .header("User-Agent", "chelsea-updater")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(UpdaterError::GitHubApi(format!(
            "Failed to download JSON metadata: HTTP {} - {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )));
    }

    let json_text = response.text().await?;
    let checksum: PrecomputedChecksum = serde_json::from_str(&json_text)?;

    Ok(checksum)
}

/// Get GitHub release information, returning the parsed release data
pub async fn get_github_release(client: Client) -> Result<GitHubRelease, UpdaterError> {
    let token = get_github_token()
        .map_err(|e| UpdaterError::Authentication(format!("Failed to get GitHub token: {}", e)))?;

    let release_response = client
        .get(REPO_RELEASES_URL)
        .header("Authorization", format!("token {}", token))
        .header("User-Agent", "chelsea-updater")
        .send()
        .await?;

    if !release_response.status().is_success() {
        return Err(UpdaterError::GitHubApi(format!(
            "Failed to get release info: HTTP {}",
            release_response.status()
        )));
    }

    let release: GitHubRelease = release_response.json().await?;
    Ok(release)
}

/// Convert a GitHubAsset to BinaryMetadata for better type tracking
pub fn asset_to_binary_metadata(
    asset: &crate::types::GitHubAsset,
    checksum: &str,
) -> crate::types::BinaryMetadata {
    crate::types::BinaryMetadata {
        asset_id: asset.id,
        size: asset.size,
        updated_at: asset.updated_at,
        created_at: asset.created_at,
        path: asset.browser_download_url.clone(),
        checksum: checksum.to_string(),
    }
}

/// Compute the SHA256 checksum and size of a local file
pub fn compute_file_checksum<P: AsRef<Path>>(
    file_path: P,
) -> Result<PrecomputedChecksum, UpdaterError> {
    let path = file_path.as_ref();

    // Read the file
    let mut file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    let file_size = metadata.len();

    // Compute SHA256 hash
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192]; // 8KB buffer for reading

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash_result = hasher.finalize();
    let sha256_hex = format!("{:x}", hash_result);

    // Get filename from path
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(PrecomputedChecksum {
        filename,
        size: file_size,
        sha256: sha256_hex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tracing::info;

    #[tokio::test]
    async fn test_get_precomputed_checksum() {
        let client = Client::new();

        match get_precomputed_checksum(client).await {
            Ok(checksum_info) => {
                info!(
                    "Successfully fetched precomputed checksum: {:#?}",
                    checksum_info
                );

                // Verify the structure
                assert_eq!(checksum_info.filename, "binary-linux");
                assert!(checksum_info.size > 0, "Size should be greater than 0");
                assert_eq!(
                    checksum_info.sha256.len(),
                    64,
                    "SHA256 hash should be 64 characters"
                );
                assert!(
                    checksum_info.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                    "Hash should be hexadecimal"
                );

                info!("   Precomputed checksum validation passed!");
                info!("   File: {}", checksum_info.filename);
                info!("   Size: {} bytes", checksum_info.size);
                info!("   SHA256: {}", checksum_info.sha256);
            }
            Err(e) => {
                info!(
                    "Warning: Failed to get precomputed checksum (expected if no AWS/GitHub access): {}",
                    e
                );
                // Don't fail the test as this requires proper AWS and GitHub setup
            }
        }
    }

    #[test]
    fn test_compute_file_checksum() {
        // Create a temporary file with known content
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let test_data = b"Hello, Chelsea updater!";
        temp_file
            .write_all(test_data)
            .expect("Failed to write test data");

        // Compute checksum
        let result = compute_file_checksum(temp_file.path()).expect("Failed to compute checksum");

        // Verify the results
        assert_eq!(result.size, test_data.len() as u64);
        assert_eq!(
            result.sha256.len(),
            64,
            "SHA256 hash should be 64 characters"
        );
        assert!(
            result.sha256.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash should be hexadecimal"
        );

        // Let's compute it manually to verify
        let mut hasher = Sha256::new();
        hasher.update(test_data);
        let expected_hash = format!("{:x}", hasher.finalize());

        assert_eq!(
            result.sha256, expected_hash,
            "SHA256 hash should match expected value"
        );

        info!("✓ File checksum computation test passed!");
        info!("  File: {}", result.filename);
        info!("  Size: {} bytes", result.size);
        info!("  SHA256: {}", result.sha256);
    }
}
