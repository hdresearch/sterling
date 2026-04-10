use std::{str::FromStr, sync::Arc};

use deadpool_postgres::{Manager, Object, Pool, RecyclingMethod};
use postgres_native_tls::MakeTlsConnector;
use tokio_postgres::{Config, NoTls, Row, Statement, ToStatement, types::ToSql};

#[macro_use]
mod macros;
mod repos;

pub use repos::*;

#[derive(Clone)]
pub struct DB(Arc<Pool>);

impl DB {
    #[tracing::instrument(skip_all)]
    pub fn new(db_str: &str) -> impl Future<Output = Result<Self, Box<dyn std::error::Error>>> {
        Self::new_with_tls(db_str, true)
    }

    pub async fn new_with_tls(
        db_str: &str,
        with_tls: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!(with_tls, "Creating database connection pool...");
        let config = Config::from_str(db_str).unwrap();

        let mng_config = deadpool_postgres::ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let manager = if !with_tls {
            Manager::from_config(config, NoTls, mng_config)
        } else {
            Manager::from_config(config, Self::build_native_tls_connector()?, mng_config)
        };
        let pool = deadpool_postgres::Pool::builder(manager)
            .max_size(16)
            .build()
            .unwrap();

        tracing::info!("done");

        Ok(DB(Arc::new(pool)))
    }

    pub async fn prepare_typed(
        &self,
        stmt: &str,
        params: &[tokio_postgres::types::Type],
    ) -> Result<Statement, DBError> {
        let obj = self.0.get().await.unwrap();
        obj.prepare_typed(stmt, params).await
    }

    pub async fn query<T>(
        &self,
        stmt: &T,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, DBError>
    where
        T: ToStatement + ?Sized,
    {
        let obj = self.0.get().await.unwrap();
        obj.query(stmt, params).await
    }

    pub async fn execute<T>(&self, stmt: &T, params: &[&(dyn ToSql + Sync)]) -> Result<u64, DBError>
    where
        T: ToStatement + ?Sized,
    {
        let obj = self.0.get().await.unwrap();
        obj.execute(stmt, params).await
    }

    pub async fn raw_obj(&self) -> Object {
        self.0.get().await.unwrap()
    }

    fn build_native_tls_connector() -> Result<MakeTlsConnector, Box<dyn std::error::Error>> {
        use native_tls::TlsConnector;
        let mut builder = TlsConnector::builder();

        builder.danger_accept_invalid_certs(true);
        let connector = builder.build()?;
        Ok(MakeTlsConnector::new(connector))
    }

    // Available in test mode or for integration tests
    #[cfg(any(test, feature = "integration-tests"))]
    pub async fn begin_for_test(&self) -> Result<(), DBError> {
        let obj = self.0.get().await.unwrap();
        obj.simple_query("BEGIN").await?;
        Ok(())
    }

    #[cfg(any(test, feature = "integration-tests"))]
    pub async fn rollback_for_test(&self) -> Result<(), DBError> {
        let obj = self.0.get().await.unwrap();
        obj.simple_query("ROLLBACK").await?;
        Ok(())
    }
}

pub type DBError = tokio_postgres::Error;
