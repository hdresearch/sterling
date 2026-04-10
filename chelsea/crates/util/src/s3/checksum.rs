use std::path::Path;

use aws_sdk_s3::Client;
use tracing::debug;

use crate::s3::{error::CompareChecksumError, read_file_from_s3};

/// Compares a local checksum to a checksum on S3, returning true if they are equal, false if not (or if the file at local_checksum_path doesn't exist)
pub async fn compare_checksums(
    client: &Client,
    bucket_name: &str,
    local_checksum_path: impl AsRef<Path>,
    key: &str,
) -> Result<bool, CompareChecksumError> {
    let local_path = local_checksum_path.as_ref();

    debug!(local_path = %local_path.display(), key, "Comparing checksums");

    // If local checksum doesn't exist, they obviously don't match
    if !local_path.exists() {
        debug!("Local checksum file does not exist at {:?}", local_path);
        return Ok(false);
    }

    // Read the local and remote checksums
    let local_bytes = tokio::fs::read(local_path).await?;
    let remote_bytes = read_file_from_s3(client, bucket_name, key).await?;

    debug!(
        "Comparing checksums: local_size={}, remote_size={}, match={}",
        local_bytes.len(),
        remote_bytes.len(),
        local_bytes == remote_bytes
    );

    Ok(local_bytes == remote_bytes)
}
