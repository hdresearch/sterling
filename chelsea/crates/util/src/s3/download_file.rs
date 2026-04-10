use std::path::Path;

use aws_sdk_s3::Client;
use futures::future::join_all;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::info;

use crate::bytes_to_mib_ceil;
use crate::s3::error::{DownloadObjectError, FileSizeError, ReadFileError};

/// Gets the size (in bytes) of a file stored in S3 at the given bucket and key.
/// Returns Ok(size) if the file exists, or Err on error/not found.
pub async fn get_s3_file_size_mib(
    client: &Client,
    bucket_name: &str,
    key: impl AsRef<str>,
) -> Result<u64, FileSizeError> {
    let head_object = client
        .head_object()
        .bucket(bucket_name)
        .key(key.as_ref())
        .send()
        .await?;

    match head_object.content_length() {
        Some(size) if size >= 0 => Ok(bytes_to_mib_ceil(size as u64)),
        Some(_) => Err(FileSizeError::NegativeContentLength),
        None => Err(FileSizeError::NoContentLength),
    }
}

/// Gets the sizes (in MiB) of multiple files stored in S3 at the given bucket and their respective keys.
/// Returns Ok(Vec<u64>) if all files are found, otherwise Err(Vec<FileSizeError>) containing the errors for each file.
pub async fn get_total_s3_file_size_mib_many(
    client: &Client,
    bucket_name: &str,
    keys: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<u64, Vec<FileSizeError>> {
    // Launch size queries in parallel
    let futures = keys
        .into_iter()
        .map(|key| get_s3_file_size_mib(client, bucket_name, key));

    let results = join_all(futures).await;

    let mut errors = Vec::new();
    let mut sizes = Vec::new();

    for result in results {
        match result {
            Ok(size) => sizes.push(size),
            Err(e) => errors.push(e),
        }
    }

    let total_size = sizes.into_iter().fold(0, |a, x| a + x);

    if errors.is_empty() {
        Ok(total_size)
    } else {
        Err(errors)
    }
}

pub async fn download_file_from_s3(
    client: &Client,
    bucket_name: &str,
    key: &str,
    dst: impl AsRef<Path>,
) -> Result<(), DownloadObjectError> {
    let dst_ref = dst.as_ref();
    info!(bucket_name, key, path = %dst_ref.display(), "Downloading file from S3...");

    // Create parent directories if they don't exist
    if let Some(parent_dir) = dst_ref.parent() {
        if !parent_dir.exists() {
            fs::create_dir_all(parent_dir).await?;
        }
    }

    // Get the object from S3
    let object = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;

    // Create the output file
    let mut file = fs::File::create(dst_ref).await?;
    let mut stream = object.body.into_async_read();
    tokio::io::copy(&mut stream, &mut file).await?;

    file.flush().await?;

    info!(bucket_name, key, out_dir = %dst_ref.display(), "Successfully downloaded file from S3");
    Ok(())
}

/// Reads a file's contents from S3 without writing to a local file
pub async fn read_file_from_s3(
    client: &Client,
    bucket_name: &str,
    key: &str,
) -> Result<Vec<u8>, ReadFileError> {
    let object = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await
        .map_err(ReadFileError::GetObject)?;

    let bytes = object
        .body
        .collect()
        .await
        .map_err(ReadFileError::CollectBody)?;

    Ok(bytes.to_vec())
}
