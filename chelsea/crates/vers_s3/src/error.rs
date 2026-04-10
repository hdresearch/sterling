use aws_sdk_s3::{
    error::SdkError,
    operation::{
        complete_multipart_upload::CompleteMultipartUploadError,
        create_multipart_upload::CreateMultipartUploadError, get_object::GetObjectError,
        head_object::HeadObjectError, upload_part::UploadPartError,
    },
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransferError {
    #[error("S3 HeadObject failed: {0}")]
    HeadObject(#[from] SdkError<HeadObjectError>),

    #[error("S3 GetObject failed: {0}")]
    GetObject(#[from] SdkError<GetObjectError>),

    #[error("S3 CreateMultipartUpload failed: {0}")]
    CreateMultipartUpload(#[from] SdkError<CreateMultipartUploadError>),

    #[error("S3 UploadPart failed: {0}")]
    UploadPart(#[from] SdkError<UploadPartError>),

    #[error("S3 CompleteMultipartUpload failed: {0}")]
    CompleteMultipartUpload(#[from] SdkError<CompleteMultipartUploadError>),

    #[error("S3 response missing content-length")]
    NoContentLength,

    #[error("S3 response returned negative content-length")]
    NegativeContentLength,

    #[error("S3 CreateMultipartUpload returned no upload ID")]
    NoUploadId,

    #[error("Failed to collect S3 response body: {0}")]
    CollectBody(#[source] aws_sdk_s3::primitives::ByteStreamError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Chunk {part} failed: {source}")]
    ChunkFailed {
        part: u64,
        #[source]
        source: Box<TransferError>,
    },

    #[error("{count} chunk(s) failed; first error: {first}")]
    MultipleChunksFailed { count: usize, first: String },
}
