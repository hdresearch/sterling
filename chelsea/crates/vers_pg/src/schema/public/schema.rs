use std::sync::Arc;

use tokio_postgres::Client;

use crate::schema::public::tables::{TableNodes, TableOrchestrators};

/// Schema `public`
pub struct SchemaPublic {
    pub nodes: TableNodes,
    pub orchestrators: TableOrchestrators,
}

impl SchemaPublic {
    pub async fn new(client: Arc<Client>) -> Result<Self, crate::Error> {
        let nodes = TableNodes::new(client.clone()).await?;
        let orchestrators = TableOrchestrators::new(client).await?;

        Ok(Self {
            nodes,
            orchestrators,
        })
    }
}
