//! Connection pool creation.

use super::PgPool;
use crate::error::DbError;

pub async fn make_pool(url: &str, label: &str) -> Result<PgPool, DbError> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(system_roots()?)
        .with_no_client_auth();
    let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config);

    let manager = bb8_postgres::PostgresConnectionManager::new_from_stringlike(url, tls)
        .map_err(|e| DbError::Pool(format!("{label}: {e}")))?;
    bb8::Pool::builder()
        .max_size(20)
        .build(manager)
        .await
        .map_err(|e| DbError::Pool(format!("{label}: {e}")))
}

fn system_roots() -> Result<rustls::RootCertStore, DbError> {
    let mut roots = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs();
    for cert in certs.certs {
        roots
            .add(cert)
            .map_err(|e| DbError::Pool(format!("bad CA cert: {e}")))?;
    }
    if roots.is_empty() {
        return Err(DbError::Pool("no system CA certificates found".into()));
    }
    Ok(roots)
}
