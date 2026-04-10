use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait AccountsRepository {
    fn get_by_id(
        &self,
        account_id: Uuid,
    ) -> impl Future<Output = Result<Option<AccountEntity>, DBError>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountEntity {
    account_id: Uuid,
    name: String,
    billing_email: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl AccountEntity {
    pub fn id(&self) -> Uuid {
        self.account_id
    }
}

impl From<Row> for AccountEntity {
    fn from(row: Row) -> Self {
        Self {
            account_id: row.get("cluster_id"),
            name: row.get("name"),
            billing_email: row.get("billing_email"),
            created_at: row.get("created_at"),
            expires_at: row.get("expires_at"),
        }
    }
}

pub struct Accounts(DB);

impl DB {
    pub fn accounts(&self) -> Accounts {
        Accounts(self.clone())
    }
}

impl AccountsRepository for Accounts {
    async fn get_by_id(&self, account_id: Uuid) -> Result<Option<AccountEntity>, DBError> {
        let tes = query_one_sql!(
            self.0,
            "SELECT * FROM accounts WHERE account_id = $1",
            &[Type::UUID],
            &[&account_id]
        )?;

        Ok(tes.map(|row| row.into()))
    }
}
