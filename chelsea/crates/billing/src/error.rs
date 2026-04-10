//! Error types for the billing crate.

/// Database-layer errors.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("connection pool error: {0}")]
    Pool(String),

    #[error("query error: {0}")]
    Query(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("credit amount must be positive")]
    InvalidCreditAmount,
}

impl From<bb8::RunError<tokio_postgres::Error>> for DbError {
    fn from(e: bb8::RunError<tokio_postgres::Error>) -> Self {
        match e {
            bb8::RunError::TimedOut => DbError::Pool(
                "connection pool timed out waiting for an available connection".into(),
            ),
            bb8::RunError::User(pg_err) => {
                DbError::Pool(format_pg_error("pool connection failed", &pg_err))
            }
        }
    }
}

impl From<tokio_postgres::Error> for DbError {
    fn from(e: tokio_postgres::Error) -> Self {
        DbError::Query(format_pg_error("query failed", &e))
    }
}

/// Format a tokio_postgres::Error with its full source chain.
fn format_pg_error(context: &str, e: &tokio_postgres::Error) -> String {
    let mut msg = format!("{context}: {e}");
    if let Some(db_err) = e.as_db_error() {
        msg.push_str(&format!(
            " [severity={}, code={}, message={}",
            db_err.severity(),
            db_err.code().code(),
            db_err.message()
        ));
        if let Some(detail) = db_err.detail() {
            msg.push_str(&format!(", detail={detail}"));
        }
        if let Some(hint) = db_err.hint() {
            msg.push_str(&format!(", hint={hint}"));
        }
        msg.push(']');
    } else if let Some(source) = std::error::Error::source(e) {
        msg.push_str(&format!(" (cause: {source})"));
    }
    msg
}

/// Stripe API errors.
#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("stripe transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("stripe responded {status}: {body}")]
    Api {
        status: reqwest::StatusCode,
        body: String,
    },
}
