use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait DomainsRepository {
    fn insert(
        &self,
        owner_id: Uuid,
        vm_id: Uuid,
        domain: &str,
    ) -> impl Future<Output = Result<DomainEntity, DomainInsertError>>;

    fn get_by_id(
        &self,
        domain_id: Uuid,
    ) -> impl Future<Output = Result<Option<DomainEntity>, DBError>>;

    fn get_by_domain(
        &self,
        domain: &str,
    ) -> impl Future<Output = Result<Option<DomainEntity>, DBError>>;

    fn list_by_owner(
        &self,
        owner_id: Uuid,
    ) -> impl Future<Output = Result<Vec<DomainEntity>, DBError>>;

    fn list_by_vm(&self, vm_id: Uuid) -> impl Future<Output = Result<Vec<DomainEntity>, DBError>>;

    fn delete(&self, domain_id: Uuid) -> impl Future<Output = Result<bool, DBError>>;
}

#[derive(Debug, Clone)]
pub struct DomainEntity {
    domain_id: Uuid,
    owner_id: Uuid,
    vm_id: Uuid,
    domain: String,
    created_at: DateTime<Utc>,
    tls_cert_id: Option<Uuid>,
    acme_http01_challenge_domain: Option<String>,
}

impl DomainEntity {
    pub fn id(&self) -> Uuid {
        self.domain_id
    }

    pub fn domain_id(&self) -> Uuid {
        self.domain_id
    }

    pub fn owner_id(&self) -> Uuid {
        self.owner_id
    }

    pub fn vm_id(&self) -> Uuid {
        self.vm_id
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn tls_cert_id(&self) -> Option<Uuid> {
        self.tls_cert_id
    }

    pub fn acme_http01_challenge_domain(&self) -> Option<&str> {
        self.acme_http01_challenge_domain.as_deref()
    }
}

impl TryFrom<Row> for DomainEntity {
    type Error = ();

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(Self {
            domain_id: row.get("domain_id"),
            owner_id: row.get("owner_id"),
            vm_id: row.get("vm_id"),
            domain: row.get("domain"),
            created_at: row.get("created_at"),
            tls_cert_id: row.get("tls_cert_id"),
            acme_http01_challenge_domain: row.get("acme_http01_challenge_domain"),
        })
    }
}

pub struct Domains(DB);

impl DB {
    pub fn domains(&self) -> Domains {
        Domains(self.clone())
    }
}

#[derive(Error, Debug)]
pub enum DomainInsertError {
    #[error("db-error: {0:?}")]
    Db(#[from] DBError),
    #[error("domain already exists")]
    DomainAlreadyExists,
}

impl DomainsRepository for Domains {
    async fn insert(
        &self,
        owner_id: Uuid,
        vm_id: Uuid,
        domain: &str,
    ) -> Result<DomainEntity, DomainInsertError> {
        let domain_lower = domain.to_lowercase();

        let maybe_row = query_one_sql!(
            self.0,
            "INSERT INTO domains (owner_id, vm_id, domain)
             VALUES ($1, $2, $3)
             RETURNING *",
            &[Type::UUID, Type::UUID, Type::TEXT],
            &[&owner_id, &vm_id, &domain_lower]
        );

        match maybe_row {
            Ok(Some(row)) => Ok(DomainEntity::try_from(row).unwrap()),
            Ok(None) => unreachable!("INSERT ... RETURNING should always return a row"),
            Err(err) => match err.as_db_error() {
                Some(db_err) => {
                    // Check for unique constraint violation (SQL state 23505)
                    // Since we're inserting into the domains table, any unique violation
                    // must be a duplicate domain error.
                    let is_unique_violation = db_err.code().code() == "23505";

                    if is_unique_violation {
                        Err(DomainInsertError::DomainAlreadyExists)
                    } else {
                        Err(DomainInsertError::Db(err))
                    }
                }
                _ => Err(DomainInsertError::Db(err)),
            },
        }
    }

    async fn get_by_id(&self, domain_id: Uuid) -> Result<Option<DomainEntity>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT * FROM domains WHERE domain_id = $1",
            &[Type::UUID],
            &[&domain_id]
        )?;
        Ok(maybe_row.map(|row| DomainEntity::try_from(row).unwrap()))
    }

    async fn get_by_domain(&self, domain: &str) -> Result<Option<DomainEntity>, DBError> {
        let domain_lower = domain.to_lowercase();
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT * FROM domains WHERE domain = $1",
            &[Type::TEXT],
            &[&domain_lower]
        )?;
        Ok(maybe_row.map(|row| DomainEntity::try_from(row).unwrap()))
    }

    async fn list_by_owner(&self, owner_id: Uuid) -> Result<Vec<DomainEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM domains WHERE owner_id = $1",
            &[Type::UUID],
            &[&owner_id]
        )?;
        Ok(rows
            .into_iter()
            .map(|r| DomainEntity::try_from(r).unwrap())
            .collect())
    }

    async fn list_by_vm(&self, vm_id: Uuid) -> Result<Vec<DomainEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM domains WHERE vm_id = $1",
            &[Type::UUID],
            &[&vm_id]
        )?;
        Ok(rows
            .into_iter()
            .map(|r| DomainEntity::try_from(r).unwrap())
            .collect())
    }

    async fn delete(&self, domain_id: Uuid) -> Result<bool, DBError> {
        let rows = execute_sql!(
            self.0,
            "DELETE FROM domains WHERE domain_id = $1",
            &[Type::UUID],
            &[&domain_id]
        )?;
        Ok(rows > 0)
    }
}
