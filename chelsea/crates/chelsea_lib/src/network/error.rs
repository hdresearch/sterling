use thiserror::Error;

#[derive(Debug, Error)]
pub enum NewVmNetworkError {
    #[error("Invalid input params: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum InitializeNetworksError {
    #[error("NetworkManager is shutting down; aborting")]
    IsShuttingDown,
    #[error("NetworkManager is down; aborting")]
    IsDown,
    #[error("NetworkManager is already running; aborting")]
    IsReady,
    #[error("NetworkManager is already starting up; aborting")]
    IsStarting,
    #[error("Error during network initialization: {0}")]
    Other(String),
}
