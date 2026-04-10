use std::sync::Arc;

use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use vers_config::VersConfig;

use crate::schema::chelsea::SchemaChelsea;
use crate::schema::public::SchemaPublic;

pub struct VersPg {
    /// Schema `chelsea`
    pub chelsea: SchemaChelsea,
    /// Schema `public`
    pub public: SchemaPublic,
}

impl VersPg {
    pub async fn new() -> Result<Self, crate::Error> {
        // Connect to the database with TLS
        let mut builder = TlsConnector::builder();
        builder.danger_accept_invalid_certs(true);
        let connector = builder
            .build()
            .map_err(|e| crate::Error::Tls(e.to_string()))?;

        let (client, connection) = tokio_postgres::connect(
            &VersConfig::global().common.database_url,
            MakeTlsConnector::new(connector),
        )
        .await?;

        // Spawn DB connection listener task
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        let client = Arc::new(client);
        let chelsea = SchemaChelsea::new(client.clone()).await?;
        let public = SchemaPublic::new(client).await?;

        Ok(Self { chelsea, public })
    }

    /// Create a VersPg with a custom connection URL (no TLS). For testing.
    #[doc(hidden)]
    pub async fn new_with_url(url: &str, _use_tls: bool) -> Result<Self, crate::Error> {
        let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;

        // Spawn DB connection listener task
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        let client = Arc::new(client);
        let chelsea = SchemaChelsea::new(client.clone()).await?;
        let public = SchemaPublic::new(client).await?;

        Ok(Self { chelsea, public })
    }
}
