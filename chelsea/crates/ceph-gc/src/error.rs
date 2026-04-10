use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("VersPg error: {0}")]
    VersPg(#[from] vers_pg::Error),
    #[error("RBD client error: {0}")]
    Rbd(#[from] ceph::RbdClientError),
}
