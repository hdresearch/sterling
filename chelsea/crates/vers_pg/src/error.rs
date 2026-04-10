use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),
    #[error("Serde JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("{0}")]
    UnexpectedValue(String),
    #[error("TLS error: {0}")]
    Tls(String),
}
