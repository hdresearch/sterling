use thiserror::Error;
use tokio::sync::broadcast;

#[derive(Debug, Error, Clone)]
pub enum VmBootError {
    #[error("The VM failed to send a 'ready' event within the permitted time.")]
    Timeout,
    #[error("VM boot was aborted by the host.")]
    Aborted,
}

#[derive(Debug, Error)]
pub enum VmReadyServiceError {
    #[error("VmReadyServiceStore error: {0}")]
    Store(#[from] VmReadyServiceStoreError),
    #[error("VM not found: {0}")]
    NotFound(String),
    #[error("Error receiving boot event: {0}")]
    Receive(#[from] broadcast::error::RecvError),
    #[error("VM failed to boot successfully: {0}")]
    BootFailed(#[from] VmBootError),
}

#[derive(Debug, Error)]
pub enum VmReadyServiceStoreError {
    #[error("DB error: {0}")]
    Db(String),
}
