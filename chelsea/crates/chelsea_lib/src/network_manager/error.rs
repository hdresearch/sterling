use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReserveNetworkError {
    #[error("There are no available networks at this time")]
    NoneAvailable,
    #[error("Internal error reserving network: {0}")]
    Other(String),
}
