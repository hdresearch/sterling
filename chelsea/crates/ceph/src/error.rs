use std::path::PathBuf;

use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum RbdClientError {
    #[error("Failed to execute rbd: {0}")]
    Exec(String),
    #[error("Rbd exited with status code {0}\nstdout:{1}\nstderr:{2}")]
    ExitCode(i32, String, String),
    #[error("Failed to find keyring at expected path: {0}")]
    KeyringNotFound(PathBuf),
    #[error("Resource not found: {0}")]
    NotFound(String),
    #[error("Rbd client error: {0}")]
    Other(String),
}
