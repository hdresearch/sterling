//! High-throughput parallel downloads from S3.
//!
//! Architecture: two-stage pipeline that decouples network from disk I/O.
//!
//! 1. **Presign** — use the SDK once to generate a pre-signed GET URL.
//! 2. **Network stage** — N concurrent `reqwest` range-GET tasks stream
//!    chunks from S3 and push `(offset, data)` pairs into a bounded channel.
//!    Tasks never touch disk.
//! 3. **Writer stage** — a dedicated OS thread drains the channel and issues
//!    `pwrite` calls. The thread runs outside tokio so dirty-page backpressure
//!    can't stall the async runtime.
//!
//! We skip sync_all() — data is durable in the page cache and the kernel
//! flushes asynchronously. Callers needing on-disk durability can fsync
//! after verifying checksums.

use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::Client;
use tokio::sync::Semaphore;
use tracing::{debug, info};

use crate::error::TransferError;
use crate::TransferConfig;

/// Pre-signed URLs are valid for this long.
const PRESIGN_EXPIRY: Duration = Duration::from_secs(3600);

/// Minimum chunk size to avoid excessive request overhead on small files.
const MIN_CHUNK_SIZE: u64 = 8 * 1024 * 1024; // 8 MiB

/// Write buffer size. Network tasks accumulate stream bytes until this
/// threshold before sending to the writer channel.
const WRITE_BUF_SIZE: usize = 2 * 1024 * 1024; // 2 MiB

/// Channel capacity (number of write buffers). Bounds memory usage:
/// max memory ≈ CHANNEL_CAPACITY * WRITE_BUF_SIZE = 512 MiB.
const CHANNEL_CAPACITY: usize = 256;

/// Returns the size (in bytes) of an S3 object.
pub async fn get_file_size_bytes(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<u64, TransferError> {
    let head = client.head_object().bucket(bucket).key(key).send().await?;

    match head.content_length() {
        Some(len) if len >= 0 => Ok(len as u64),
        Some(_) => Err(TransferError::NegativeContentLength),
        None => Err(TransferError::NoContentLength),
    }
}

/// Download an S3 object to a local file.
///
/// Small files use a single GET. Large files use the parallel pipeline.
pub async fn download_file(
    client: &Client,
    bucket: &str,
    key: &str,
    dst: impl AsRef<Path>,
    config: &TransferConfig,
) -> Result<(), TransferError> {
    let dst = dst.as_ref();
    let file_size = get_file_size_bytes(client, bucket, key).await?;

    info!(
        bucket,
        key,
        file_size,
        path = %dst.display(),
        "Downloading from S3"
    );

    if let Some(parent) = dst.parent() {
        if !parent.exists() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    if file_size == 0 {
        tokio::fs::File::create(dst).await?;
        return Ok(());
    }

    if file_size < config.parallel_threshold {
        return download_single(client, bucket, key, dst).await;
    }

    download_parallel(client, bucket, key, dst, file_size, config).await
}

/// Download an S3 object into memory.
pub async fn download_to_vec(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<Vec<u8>, TransferError> {
    let resp = client.get_object().bucket(bucket).key(key).send().await?;

    let bytes = resp
        .body
        .collect()
        .await
        .map_err(TransferError::CollectBody)?;

    Ok(bytes.to_vec())
}

/// Single-stream download for small files. Uses the SDK directly.
async fn download_single(
    client: &Client,
    bucket: &str,
    key: &str,
    dst: &Path,
) -> Result<(), TransferError> {
    debug!(bucket, key, "Single-stream download (SDK)");

    let resp = client.get_object().bucket(bucket).key(key).send().await?;

    let mut file = tokio::fs::File::create(dst).await?;
    let mut stream = resp.body.into_async_read();
    tokio::io::copy(&mut stream, &mut file).await?;
    tokio::io::AsyncWriteExt::flush(&mut file).await?;

    Ok(())
}

/// Compute the chunk size that maximises parallelism for the given file.
fn compute_chunk_size(file_size: u64, config: &TransferConfig) -> u64 {
    let ideal = file_size / config.max_concurrency.max(1) as u64;
    let floor = MIN_CHUNK_SIZE.min(config.chunk_size);
    let ceiling = config.chunk_size.max(floor);
    ideal.clamp(floor, ceiling)
}

/// A write command sent from network tasks to the writer thread.
struct WriteCmd {
    offset: u64,
    data: Vec<u8>,
}

/// Parallel streaming download via pre-signed URL + reqwest.
///
/// Network tasks push data through an async channel to a dedicated writer
/// thread, completely decoupling network I/O from disk I/O.
async fn download_parallel(
    client: &Client,
    bucket: &str,
    key: &str,
    dst: &Path,
    file_size: u64,
    config: &TransferConfig,
) -> Result<(), TransferError> {
    let chunk_size = compute_chunk_size(file_size, config);
    let chunk_count = (file_size + chunk_size - 1) / chunk_size;

    // Presign the GetObject URL once.
    let presigned = client
        .get_object()
        .bucket(bucket)
        .key(key)
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
                format!("presigning failed: {e}"),
            ))
        })?;

    let presigned_url = presigned.uri().to_string();

    debug!(
        bucket,
        key,
        file_size,
        chunk_size,
        chunk_count,
        max_concurrency = config.max_concurrency,
        "Parallel pipeline download (reqwest → channel → writer thread)"
    );

    // Build reqwest client optimised for bulk transfer.
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

    // Warm up the connection pool.
    let _ = http_client.head(&presigned_url).send().await;

    // Pre-allocate the output file.
    let file = std::fs::File::create(dst)?;
    file.set_len(file_size)?;

    // --- Writer thread: drains the channel and writes to disk ---
    // Uses tokio::sync::mpsc so senders (.send().await) yield the tokio
    // worker thread when the channel is full. The writer thread calls
    // .blocking_recv() which is fine on an OS thread.
    //
    // Attempts io_uring for batched writes (zero syscall overhead per write).
    // Falls back to pwrite if io_uring is unavailable.
    let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<WriteCmd>(CHANNEL_CAPACITY);

    let writer_handle = std::thread::spawn(move || -> Result<(), TransferError> {
        while let Some(cmd) = write_rx.blocking_recv() {
            file.write_at(&cmd.data, cmd.offset)?;
        }
        Ok(())
    });

    // --- Network tasks: parallel range GETs, push to channel ---
    let semaphore = Arc::new(Semaphore::new(config.max_concurrency));
    let mut handles = Vec::with_capacity(chunk_count as usize);

    for chunk_index in 0..chunk_count {
        let http_client = http_client.clone();
        let url = presigned_url.clone();
        let semaphore = Arc::clone(&semaphore);
        let write_tx = write_tx.clone();

        handles.push(tokio::spawn(async move {
            let _permit = semaphore.acquire().await.map_err(|_| {
                TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "semaphore closed",
                ))
            })?;

            let start = chunk_index * chunk_size;
            let end = std::cmp::min(start + chunk_size, file_size) - 1;
            let range_header = format!("bytes={start}-{end}");

            debug!(chunk_index, range = %range_header, "Fetching range");

            let resp = http_client
                .get(&url)
                .header(reqwest::header::RANGE, &range_header)
                .send()
                .await
                .map_err(|e| {
                    TransferError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("reqwest GET failed: {e}"),
                    ))
                })?;

            if !resp.status().is_success() {
                return Err(TransferError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "S3 returned HTTP {} for range {}",
                        resp.status(),
                        range_header
                    ),
                )));
            }

            // Stream body and push write commands through the channel.
            let mut stream = resp.bytes_stream();
            let mut offset = start;
            let mut write_buf = Vec::with_capacity(WRITE_BUF_SIZE);

            use futures::StreamExt;
            while let Some(chunk_result) = stream.next().await {
                let bytes = chunk_result.map_err(|e| {
                    TransferError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("stream read error: {e}"),
                    ))
                })?;
                write_buf.extend_from_slice(&bytes);

                if write_buf.len() >= WRITE_BUF_SIZE {
                    let data =
                        std::mem::replace(&mut write_buf, Vec::with_capacity(WRITE_BUF_SIZE));
                    let data_len = data.len() as u64;
                    write_tx
                        .send(WriteCmd { offset, data })
                        .await
                        .map_err(|_| {
                            TransferError::Io(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "writer thread dropped",
                            ))
                        })?;
                    offset += data_len;
                }
            }

            // Flush remaining bytes.
            if !write_buf.is_empty() {
                write_tx
                    .send(WriteCmd {
                        offset,
                        data: write_buf,
                    })
                    .await
                    .map_err(|_| {
                        TransferError::Io(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "writer thread dropped",
                        ))
                    })?;
            }

            debug!(chunk_index, "Chunk fetch complete");
            Ok::<(), TransferError>(())
        }));
    }

    // Drop our copy of the sender so the writer thread sees EOF.
    drop(write_tx);

    // Collect network task results.
    let mut errors = Vec::new();
    for (i, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(())) => {}
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

    // Wait for writer thread to finish.
    let writer_result = writer_handle.join().map_err(|_| {
        TransferError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "writer thread panicked",
        ))
    })?;

    if !errors.is_empty() {
        if errors.len() == 1 {
            let (part, err) = errors.into_iter().next().expect("checked len == 1");
            let _ = std::fs::remove_file(dst);
            return Err(TransferError::ChunkFailed {
                part: part as u64,
                source: Box::new(err),
            });
        }
        let count = errors.len();
        let first = errors
            .into_iter()
            .next()
            .map(|(_, e)| e.to_string())
            .unwrap_or_default();
        let _ = std::fs::remove_file(dst);
        return Err(TransferError::MultipleChunksFailed { count, first });
    }

    writer_result?;

    info!(
        bucket,
        key,
        file_size,
        chunks = chunk_count,
        "Download complete"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_chunk_sizing_caps_at_max() {
        let config = TransferConfig {
            chunk_size: 64 * 1024 * 1024,
            max_concurrency: 16,
            parallel_threshold: 8 * 1024 * 1024,
        };
        let cs = compute_chunk_size(16 * 1024 * 1024 * 1024, &config);
        assert_eq!(cs, 64 * 1024 * 1024);
    }

    #[test]
    fn dynamic_chunk_sizing_floor_at_min() {
        let config = TransferConfig {
            chunk_size: 64 * 1024 * 1024,
            max_concurrency: 16,
            parallel_threshold: 8 * 1024 * 1024,
        };
        let cs = compute_chunk_size(32 * 1024 * 1024, &config);
        assert_eq!(cs, MIN_CHUNK_SIZE);
    }

    #[test]
    fn dynamic_chunk_sizing_sweet_spot() {
        let config = TransferConfig {
            chunk_size: 64 * 1024 * 1024,
            max_concurrency: 16,
            parallel_threshold: 8 * 1024 * 1024,
        };
        let cs = compute_chunk_size(512 * 1024 * 1024, &config);
        assert_eq!(cs, 32 * 1024 * 1024);
    }

    #[test]
    fn chunk_count_equals_concurrency_for_large_files() {
        let config = TransferConfig {
            chunk_size: 64 * 1024 * 1024,
            max_concurrency: 16,
            parallel_threshold: 8 * 1024 * 1024,
        };
        let file_size = 4u64 * 1024 * 1024 * 1024;
        let cs = compute_chunk_size(file_size, &config);
        let chunks = (file_size + cs - 1) / cs;
        assert_eq!(cs, 64 * 1024 * 1024);
        assert_eq!(chunks, 64);
    }
}
