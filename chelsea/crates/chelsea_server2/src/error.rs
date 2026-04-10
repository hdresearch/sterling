use axum::http::StatusCode;
use chelsea_lib::{store_error::StoreError, vm_manager::error::VmManagerError};
use dto_lib::chelsea_server2::error::ChelseaServerError;
use thiserror::Error;

/// Associate HTTP status codes with rich error types.
pub trait ErrorStatusCode {
    fn status_code(&self) -> StatusCode;
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error(transparent)]
    CreateVm(#[from] CreateVmError),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl ErrorStatusCode for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::CreateVm(inner) => inner.status_code(),
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<ApiError> for ChelseaServerError {
    fn from(value: ApiError) -> Self {
        ChelseaServerError::new(value.to_string(), value.status_code())
    }
}

impl From<StoreError> for ApiError {
    fn from(value: StoreError) -> Self {
        Self::internal(value.to_string())
    }
}

impl From<VmManagerError> for ApiError {
    fn from(value: VmManagerError) -> Self {
        Self::internal(format!("{value:#}"))
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self::internal(value.to_string())
    }
}

#[derive(Debug, Error)]
pub enum CreateVmError {
    #[error("{0}")]
    KernelNotFound(String),
    #[error("{0}")]
    ImageNotFound(String),
    #[error("{0}")]
    TooSmall(String),
    #[error("{0}")]
    Other(String),
}

impl CreateVmError {
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }
}

impl ErrorStatusCode for CreateVmError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::KernelNotFound(_) | Self::ImageNotFound(_) => StatusCode::NOT_FOUND,
            Self::TooSmall(_) => StatusCode::BAD_REQUEST,
            Self::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
