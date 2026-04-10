//! High-throughput parallel multipart uploads to S3.
//!
//! Architecture mirrors the download pipeline but in reverse:
//!
//! 1. **CreateMultipartUpload** — SDK call to get an upload_id.
//! 2. **Presign** each UploadPart URL — one per chunk.
//! 3. **Reader pool** — M OS threads read file chunks via `pread` and push
//!    data through a channel. Runs outside tokio so disk I/O can't stall
//!    the async runtime.
//! 4. **Network stage** — N concurrent `reqwest` PUT tasks consume from
//!    the channel and upload each part via the presigned URL.
//! 5. **CompleteMultipartUpload** — SDK call with the ETags.
//!
//! The presigned URL approach bypasses the SDK for the data path, eliminating
//! per-request middleware overhead.

use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::error::TransferError;
use crate::TransferConfig;

/// Pre-signed URLs are valid for this long.
const PRESIGN_EXPIRY: Duration = Duration::from_secs(3600);

/// Upload a local file to S3 using parallel multipart upload.
///
/// For small files (below `config.parallel_threshold`), uses a simple
/// single-request PUT.
pub async fn upload_file(
    client: &Client,
    bucket: &str,
    key: &str,
    src: impl AsRef<Path>,
    config: &TransferConfig,
) -> Result<(), TransferError> {
    let src = src.as_ref();
    let metadata = tokio::fs::metadata(src).await?;
    let file_size = metadata.len();

    info!(
        bucket,
        key,
        file_size,
        path = %src.display(),
        "Uploading to S3"
    );

    if file_size == 0 {
        client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(ByteStream::from(Vec::new()))
            .content_length(0)
            .send()
            .await
            .map_err(|e| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
        return Ok(());
    }

    if file_size < config.parallel_threshold {
        return upload_single(client, bucket, key, src, file_size).await;
    }

    upload_multipart(client, bucket, key, src, file_size, config).await
}

/// Simple single-request upload for small files.
async fn upload_single(
    client: &Client,
    bucket: &str,
    key: &str,
    src: &Path,
    file_size: u64,
) -> Result<(), TransferError> {
    debug!(bucket, key, file_size, "Single-request upload");

    let body = ByteStream::from_path(src)
        .await
        .map_err(|e| TransferError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        .content_length(file_size as i64)
        .send()
        .await
        .map_err(|e| {
            TransferError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

    Ok(())
}

/// Minimum chunk size for uploads (S3 requires >= 5 MiB for multipart parts,
/// except the last part).
const MIN_UPLOAD_CHUNK: u64 = 8 * 1024 * 1024; // 8 MiB

/// Compute upload chunk size, similar to download.
fn compute_upload_chunk_size(file_size: u64, config: &TransferConfig) -> u64 {
    let ideal = file_size / config.max_concurrency.max(1) as u64;
    let floor = MIN_UPLOAD_CHUNK.min(config.chunk_size);
    let ceiling = config.chunk_size.max(floor);
    ideal.clamp(floor, ceiling)
}

/// Part info for a chunk to be uploaded.
struct PartSpec {
    part_number: i32,
    offset: u64,
    length: u64,
    presigned_url: String,
}

/// Parallel multipart upload via presigned URLs + reqwest.
async fn upload_multipart(
    client: &Client,
    bucket: &str,
    key: &str,
    src: &Path,
    file_size: u64,
    config: &TransferConfig,
) -> Result<(), TransferError> {
    let chunk_size = compute_upload_chunk_size(file_size, config);
    let chunk_count = (file_size + chunk_size - 1) / chunk_size;

    debug!(
        bucket,
        key,
        file_size,
        chunk_size,
        chunk_count,
        max_concurrency = config.max_concurrency,
        "Parallel multipart upload (presigned URLs + reqwest)"
    );

    // Create multipart upload
    let create_resp = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .send()
        .await?;

    let upload_id = create_resp
        .upload_id()
        .ok_or(TransferError::NoUploadId)?
        .to_string();

    // Presign all UploadPart URLs up front.
    let mut part_specs = Vec::with_capacity(chunk_count as usize);
    for chunk_index in 0..chunk_count {
        let part_number = (chunk_index as i32) + 1;
        let offset = chunk_index * chunk_size;
        let length = std::cmp::min(chunk_size, file_size - offset);

        let presigned = client
            .upload_part()
            .bucket(bucket)
            .key(key)
            .upload_id(&upload_id)
            .part_number(part_number)
            .presigned(PresigningConfig::expires_in(PRESIGN_EXPIRY).map_err(|e| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("presigning config error: {e}"),
                ))
            })?)
            .await
            .map_err(|e| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("presigning upload_part failed: {e}"),
                ))
            })?;

        part_specs.push(PartSpec {
            part_number,
            offset,
            length,
            presigned_url: presigned.uri().to_string(),
        });
    }

    // On failure, abort the multipart upload.
    let result = upload_parts_presigned(src, file_size, &part_specs, config).await;

    match result {
        Ok(parts) => {
            let completed = CompletedMultipartUpload::builder()
                .set_parts(Some(parts))
                .build();

            client
                .complete_multipart_upload()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .multipart_upload(completed)
                .send()
                .await?;

            info!(
                bucket,
                key,
                file_size,
                chunks = chunk_count,
                "Upload complete"
            );
            Ok(())
        }
        Err(e) => {
            if let Err(abort_err) = client
                .abort_multipart_upload()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .send()
                .await
            {
                warn!(error = %abort_err, "Failed to abort multipart upload");
            }
            Err(e)
        }
    }
}

/// Upload all parts using presigned URLs and reqwest.
///
/// Reads file chunks on blocking threads and uploads via reqwest,
/// bypassing the SDK for the data path.
async fn upload_parts_presigned(
    src: &Path,
    _file_size: u64,
    part_specs: &[PartSpec],
    config: &TransferConfig,
) -> Result<Vec<CompletedPart>, TransferError> {
    let file = Arc::new(std::fs::File::open(src).map_err(TransferError::Io)?);

    // Build reqwest client for uploads.
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(config.max_concurrency)
        .tcp_nodelay(true)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| {
            TransferError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to build reqwest client: {e}"),
            ))
        })?;

    let semaphore = Arc::new(Semaphore::new(config.max_concurrency));
    let mut handles = Vec::with_capacity(part_specs.len());

    for spec in part_specs {
        let http_client = http_client.clone();
        let semaphore = Arc::clone(&semaphore);
        let file = Arc::clone(&file);
        let url = spec.presigned_url.clone();
        let part_number = spec.part_number;
        let offset = spec.offset;
        let length = spec.length;

        handles.push(tokio::spawn(async move {
            let _permit = semaphore.acquire().await.map_err(|_| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "semaphore closed",
                ))
            })?;

            debug!(part_number, offset, length, "Reading + uploading part");

            // Read the chunk from disk on a blocking thread.
            let data = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, TransferError> {
                let mut buf = vec![0u8; length as usize];
                file.read_at(&mut buf, offset)?;
                Ok(buf)
            })
            .await
            .map_err(|e| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("read task join error: {e}"),
                ))
            })?
            .map_err(|e| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("pread failed: {e}"),
                ))
            })?;

            // Upload via presigned PUT.
            let resp = http_client
                .put(&url)
                .header(reqwest::header::CONTENT_LENGTH, length)
                .body(data)
                .send()
                .await
                .map_err(|e| {
                    TransferError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("reqwest PUT failed: {e}"),
                    ))
                })?;

            if !resp.status().is_success() {
                return Err(TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "S3 returned HTTP {} for part {}",
                        resp.status(),
                        part_number,
                    ),
                )));
            }

            // Extract ETag from response header.
            let etag = resp
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            debug!(part_number, %etag, "Part upload complete");

            Ok::<CompletedPart, TransferError>(
                CompletedPart::builder()
                    .e_tag(etag)
                    .part_number(part_number)
                    .build(),
            )
        }));
    }

    let mut parts = Vec::with_capacity(handles.len());
    let mut errors = Vec::new();

    for (i, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(part)) => parts.push(part),
            Ok(Err(e)) => errors.push((i, e)),
            Err(join_err) => errors.push((
                i,
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    join_err.to_string(),
                )),
            )),
        }
    }

    if !errors.is_empty() {
        let count = errors.len();
        let first = errors
            .into_iter()
            .next()
            .map(|(_, e)| e.to_string())
            .unwrap_or_default();
        return Err(TransferError::MultipleChunksFailed { count, first });
    }

    // Sort by part number — S3 requires ordered parts.
    parts.sort_by_key(|p| p.part_number());

    Ok(parts)
}
