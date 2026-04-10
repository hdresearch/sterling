use anyhow::Error as AnyhowError;
use aws_sdk_s3::{
    error::SdkError,
    operation::{
        get_object::GetObjectError, head_object::HeadObjectError,
        list_objects_v2::ListObjectsV2Error,
    },
};
use thiserror::Error;
use tokio::task::JoinError;

#[derive(Error, Debug)]
pub enum FileSizeError {
    #[error("Negative content length returned")]
    NegativeContentLength,
    #[error("No content length returned")]
    NoContentLength,
    #[error(transparent)]
    HeadObjectError(#[from] SdkError<HeadObjectError>),
}

#[derive(Error, Debug)]
pub enum DownloadObjectError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    GetObjectError(#[from] SdkError<GetObjectError>),
}

#[derive(Error, Debug)]
pub enum ReadFileError {
    #[error("Failed to get object: {0}")]
    GetObject(SdkError<GetObjectError>),
    #[error("Failed to collect body: {0}")]
    CollectBody(aws_sdk_s3::primitives::ByteStreamError),
}

#[derive(Error, Debug)]
pub enum ListObjectsError {
    #[error(transparent)]
    ListObjectsV2(#[from] SdkError<ListObjectsV2Error>),
}

#[derive(Error, Debug)]
pub enum DeletePrefixError {
    #[error(transparent)]
    List(#[from] ListObjectsError),
    #[error("failed to delete S3 objects: {0}")]
    DeleteObjects(#[from] AnyhowError),
}

#[derive(Error, Debug)]
pub enum DownloadDirectoryError {
    #[error("One or more tasks failed: {0:?}")]
    TaskErrors(Vec<DownloadDirectoryTaskError>),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    ListObjects(#[from] ListObjectsError),
}

#[derive(Error, Debug)]
pub enum DownloadDirectoryTaskError {
    #[error("Failed to extract filename from key: {0}")]
    ExtractFilenameFromKey(String),
    #[error(transparent)]
    DownloadObjectError(#[from] DownloadObjectError),
    #[error(transparent)]
    JoinError(#[from] JoinError),
}

#[derive(Error, Debug)]
pub enum CompareChecksumError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    ReadFileError(#[from] ReadFileError),
}
