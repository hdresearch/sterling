//! `vers_s3` — a purpose-built S3 transfer engine for Vers.
//!
//! Provides high-throughput parallel uploads and downloads by splitting large
//! objects into byte-range chunks and transferring them concurrently. Metadata
//! operations (HEAD, LIST, DELETE) use the AWS SDK directly.
//!
//! # Design
//!
//! S3 caps single-stream throughput at ~100-200 MB/s. For the multi-GiB VM
//! snapshots that Vers moves during sleep/wake, this is the bottleneck. By
//! splitting into N parallel streams we can approach the EC2 instance's
//! network bandwidth limit instead.
//!
//! The transfer engine is configurable via [`TransferConfig`] and all
//! operations accept an `&aws_sdk_s3::Client` — callers own client lifecycle.

mod download;
mod error;
mod upload;

pub use download::{download_file, download_to_vec, get_file_size_bytes};
pub use error::TransferError;
pub use upload::upload_file;

/// Configuration for parallel transfers.
#[derive(Debug, Clone)]
pub struct TransferConfig {
    /// Chunk size in bytes for parallel transfers.
    /// Default: 64 MiB.
    pub chunk_size: u64,

    /// Maximum number of concurrent chunk transfers.
    /// Default: 16.
    pub max_concurrency: usize,

    /// Files smaller than this threshold (in bytes) use a single GET/PUT
    /// instead of parallel transfer. Default: 16 MiB.
    pub parallel_threshold: u64,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64 * 1024 * 1024, // 64 MiB — larger chunks = fewer HTTP round-trips
            max_concurrency: 64,          // Saturate 25 Gbps with many parallel streams
            parallel_threshold: 8 * 1024 * 1024, // 8 MiB
        }
    }
}
