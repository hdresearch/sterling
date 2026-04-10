use thiserror::Error;

#[derive(Debug, Error)]
pub enum EnsureSpaceError {
    #[error(
        "There is not enough space allocated to the S3CommitStore to download the files. Required: {space_required_mib} MiB, Available: {cache_size_mib} MiB"
    )]
    CacheNotLargeEnough {
        space_required_mib: u32,
        cache_size_mib: u32,
    },
    #[error("Error while calculating S3CommitStore cache utilization: {0}")]
    GetCacheUtilization(#[from] GetCacheUtilizationError),
    #[error("Error while deleting file in S3CommitStore cache: {0}")]
    DeleteOldestFile(#[from] DeleteOldestFileError),
}

#[derive(Debug, Error)]
pub enum DeleteOldestFileError {
    #[error("IO error while deleting oldest file in S3CommitStore cache: {0}")]
    Io(#[from] std::io::Error),
    #[error("There are no files left to delete")]
    NoFiles,
}

#[derive(Debug, Error)]
pub enum GetCacheUtilizationError {
    #[error("IO error while determining cache utilization: {0}")]
    Io(#[from] std::io::Error),
}
