use anyhow::{anyhow, bail};
use aws_sdk_s3::primitives::{ByteStream, Length};
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use std::{
    error::Error,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs::File;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use crate::{defer::DeferAsync, join_errors, s3::get_s3_client};

/// Uploads the specified files to S3, returning their S3 keys
pub async fn upload_files_with_prefix(
    bucket_name: &str,
    file_paths: impl Iterator<Item = PathBuf>,
    prefix: &str,
) -> anyhow::Result<Vec<String>> {
    let upload_futures = file_paths.map(|path| {
        let bucket_name = bucket_name.to_string();
        let prefix = prefix.to_string();
        async move {
            if !path.exists() {
                bail!("File does not exist: {}", path.display());
            }

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(anyhow::anyhow!("Invalid filename"))?;

            let s3_key = format!("{}/{}", prefix, file_name);
            debug!(
                "Uploading {} to s3://{}/{}",
                path.display(),
                bucket_name,
                s3_key
            );

            upload_with_cli(&bucket_name, &s3_key, &path).await?;

            debug!("Successfully uploaded {}", s3_key);
            Ok::<String, anyhow::Error>(s3_key)
        }
    });

    let upload_results = futures::future::join_all(upload_futures)
        .await
        .into_iter()
        .collect::<Vec<_>>();

    let mut upload_errors = Vec::new();
    let mut uploaded_keys = Vec::new();
    for upload_result in upload_results {
        match upload_result {
            Ok(uploaded_key) => uploaded_keys.push(uploaded_key),
            Err(upload_error) => upload_errors.push(upload_error),
        }
    }

    match upload_errors.is_empty() {
        false => Err(anyhow!(
            "One or more errors while uploading to S3: {}",
            join_errors(&upload_errors, "; ")
        )),
        true => Ok(uploaded_keys),
    }
}

/// Upload a file to S3 using the simple API
/// Marked as dead code for now; we are using the CLI for all uploads until we can optimize the Rust code to be faster.
/// See https://github.com/hdresearch/chelsea/pull/793
#[allow(dead_code)]
async fn upload_simple(bucket_name: &str, key: &str, file_path: &Path) -> anyhow::Result<()> {
    let client = get_s3_client().await;

    // Open the file
    let file = File::open(file_path).await?;
    let file_size = file.metadata().await?.len();

    // Create a ByteStream from the file
    let body = ByteStream::from_path(file_path).await?;

    // Upload it
    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(body)
        .content_length(file_size as i64)
        .send()
        .await?;

    debug!(
        "Uploaded {} bytes to s3://{}/{}",
        file_size, bucket_name, key
    );
    Ok(())
}

/// Options for the multipart upload function
/// Marked as dead code for now; we are using the CLI for all uploads until we can optimize the Rust code to be faster.
/// See https://github.com/hdresearch/chelsea/pull/793
#[allow(dead_code)]
pub struct MultipartUploadOptions {
    /// Chunk size, in bytes (default: 128 MB)
    pub chunk_size: u64,
    /// The maximum number of chunks permitted (default: 10,000 - S3's limit)
    pub max_chunks: u64,
    /// Maximum number of parallel chunk uploads (default: 16)
    pub max_parallel_uploads: usize,
}

impl Default for MultipartUploadOptions {
    fn default() -> Self {
        Self {
            chunk_size: 128 * 1024 * 1024, // 128 MB
            max_chunks: 10_000,            // S3's actual limit
            max_parallel_uploads: 16,
        }
    }
}

/// Upload a file to S3 using the multipart API with parallel chunk uploads
/// Marked as dead code for now; we are using the CLI for all uploads until we can optimize the Rust code to be faster.
/// See https://github.com/hdresearch/chelsea/pull/793
#[allow(dead_code)]
async fn upload_multipart(
    bucket_name: &str,
    key: &str,
    file_path: &Path,
    options: &MultipartUploadOptions,
) -> anyhow::Result<()> {
    let client = get_s3_client().await;

    // Create the multipart upload request
    let multipart_upload_res = client
        .create_multipart_upload()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;

    let upload_id = multipart_upload_res
        .upload_id()
        .ok_or(anyhow!("Missing upload_id after CreateMultipartUpload"))?
        .to_string();

    // Defer aborting the upload request
    let mut defer = DeferAsync::new();
    let abort_bucket = bucket_name.to_string();
    let abort_key = key.to_string();
    let abort_upload_id = upload_id.clone();
    defer.defer(async move {
        let client = get_s3_client().await;

        let multipart_abort_res = client
            .abort_multipart_upload()
            .bucket(&abort_bucket)
            .key(&abort_key)
            .upload_id(&abort_upload_id)
            .send()
            .await;

        if let Err(error) = multipart_abort_res {
            warn!(error = ?error.source(), upload_id = %abort_upload_id, "Failed to abort multipart upload");
        }
    });

    // Calculate the required number of chunks, as well as the size of the final chunk
    let file_size = file_path.metadata()?.size();

    let mut chunk_count = (file_size / options.chunk_size) + 1;
    let mut size_of_last_chunk = file_size % options.chunk_size;
    if size_of_last_chunk == 0 {
        size_of_last_chunk = options.chunk_size;
        chunk_count -= 1;
    }

    if file_size == 0 {
        bail!("Size of file {} is 0", file_path.display());
    }
    if chunk_count > options.max_chunks {
        bail!(
            "Upload of file {} would require {} chunks; max: {} (chunk size: {})",
            file_path.display(),
            chunk_count,
            options.max_chunks,
            options.chunk_size
        );
    }

    debug!(
        "Uploading {} in {} chunks of {} bytes each (last chunk: {} bytes)",
        file_path.display(),
        chunk_count,
        options.chunk_size,
        size_of_last_chunk
    );

    // Upload all chunks in parallel (with semaphore limiting concurrency)
    let semaphore = Arc::new(Semaphore::new(options.max_parallel_uploads));

    let upload_futures = (0..chunk_count).map(|chunk_index| {
        let semaphore = semaphore.clone();
        let bucket_name = bucket_name.to_string();
        let key = key.to_string();
        let upload_id = upload_id.clone();
        let file_path = file_path.to_path_buf();
        let chunk_size = options.chunk_size;

        async move {
            // Acquire semaphore permit
            let _permit = semaphore.acquire().await.map_err(anyhow::Error::from)?;

            let client = get_s3_client().await;

            let current_chunk_size = if chunk_count - 1 == chunk_index {
                size_of_last_chunk
            } else {
                chunk_size
            };

            // Construct ByteStream to be read from
            let stream = ByteStream::read_from()
                .path(&file_path)
                .offset(chunk_index * chunk_size)
                .length(Length::Exact(current_chunk_size))
                .build()
                .await?;

            // Chunk index needs to start at 0, but part numbers start at 1.
            let part_number = (chunk_index as i32) + 1;

            debug!(
                "Uploading part {} of {} (offset: {}, size: {})",
                part_number,
                chunk_count,
                chunk_index * chunk_size,
                current_chunk_size
            );

            let upload_part_res = client
                .upload_part()
                .key(&key)
                .bucket(&bucket_name)
                .upload_id(&upload_id)
                .body(stream)
                .part_number(part_number)
                .send()
                .await?;

            Ok::<CompletedPart, anyhow::Error>(
                CompletedPart::builder()
                    .e_tag(upload_part_res.e_tag.unwrap_or_default())
                    .part_number(part_number)
                    .build(),
            )
        }
    });

    let upload_results = futures::future::join_all(upload_futures).await;

    // Collect results and check for errors
    let mut upload_parts = Vec::new();
    for result in upload_results {
        upload_parts.push(result?);
    }

    debug!("All {} parts uploaded successfully", upload_parts.len());

    // Complete the multipart upload
    let completed_multipart_upload = CompletedMultipartUpload::builder()
        .set_parts(Some(upload_parts))
        .build();

    client
        .complete_multipart_upload()
        .bucket(bucket_name)
        .key(key)
        .multipart_upload(completed_multipart_upload)
        .upload_id(&upload_id)
        .send()
        .await?;

    defer.commit();
    debug!(
        "Successfully completed multipart upload: s3://{}/{}",
        bucket_name, key
    );

    Ok(())
}

/// Upload a file to S3 using the AWS CLI
async fn upload_with_cli(bucket_name: &str, key: &str, file_path: &Path) -> anyhow::Result<()> {
    use tokio::process::Command;

    let s3_uri = format!("s3://{}/{}", bucket_name, key);

    debug!(
        "Uploading {} to {} using AWS CLI",
        file_path.display(),
        s3_uri
    );

    let output = Command::new("aws")
        .arg("s3")
        .arg("cp")
        .arg(file_path)
        .arg(&s3_uri)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("AWS CLI upload failed: {}", stderr);
    }

    debug!("Successfully uploaded {} using AWS CLI", s3_uri);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    #[ignore]
    async fn test_upload_file() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("test_file.txt");

        let mut file = tokio::fs::File::create(&file_path).await?;
        file.write_all(b"Hello, S3!").await?;
        file.flush().await?;
        drop(file);

        let bucket_name = "vers-commits--use1-az4--x-s3";
        let key = "test/test_file.txt";

        upload_simple(bucket_name, key, &file_path).await?;

        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_upload_multipart() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("large_test_file.bin");

        // Create a 50MB file for testing
        let mut file = tokio::fs::File::create(&file_path).await?;
        let chunk = vec![0u8; 1024 * 1024]; // 1MB
        for _ in 0..50 {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);

        let bucket_name = "vers-commits--use1-az4--x-s3";
        let key = "test/large_test_file.bin";

        let options = MultipartUploadOptions {
            chunk_size: 10 * 1024 * 1024, // 10MB chunks
            max_parallel_uploads: 4,
            ..Default::default()
        };

        upload_multipart(bucket_name, key, &file_path, &options).await?;

        Ok(())
    }
}
