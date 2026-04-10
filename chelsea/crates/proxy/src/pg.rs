use std::net::{IpAddr, Ipv6Addr};
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result};
use bb8_postgres::{
    PostgresConnectionManager,
    bb8::{Pool, PooledConnection},
};
use chrono::{DateTime, Utc};
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use tokio_postgres::{Row, ToStatement, types::ToSql};
use uuid::Uuid;

static DB_MANAGER_INSTANCE: OnceLock<DBManager> = OnceLock::new();

pub type DBConnection<'a> = PooledConnection<'a, PostgresConnectionManager<MakeTlsConnector>>;
pub type DBPool = Pool<PostgresConnectionManager<MakeTlsConnector>>;

/// Normalize a domain name to lowercase for case-insensitive storage and lookup.
/// DNS is case-insensitive per RFC 1035, so we store all domains in lowercase.
fn normalize_domain(domain: &str) -> String {
    domain.to_ascii_lowercase()
}

pub struct DBOptions {
    pub pg_params: String,
    pub pool_max_size: u32,
}

pub struct DBManager {
    pool: DBPool,
}

impl DBManager {
    pub async fn get() -> &'static DBManager {
        DB_MANAGER_INSTANCE.get().unwrap()
    }

    async fn new(config: DBOptions) -> Result<Self, anyhow::Error> {
        let DBOptions {
            pg_params,
            pool_max_size,
        } = config;

        tracing::debug!(
            pool_max_size = pool_max_size,
            "Creating database connection pool"
        );

        let mut builder = TlsConnector::builder();
        builder.danger_accept_invalid_certs(true);
        let connector = builder.build()?;

        tracing::debug!("Building PostgresConnectionManager");
        let manager = PostgresConnectionManager::new_from_stringlike(
            pg_params,
            MakeTlsConnector::new(connector),
        )
        .expect("Unable to build PostgresConnectionManager");

        tracing::debug!(pool_max_size = pool_max_size, "Building connection pool");
        let pool = Pool::builder()
            .max_size(pool_max_size)
            .build(manager)
            .await
            .context("Postgres error")?;

        tracing::info!(
            pool_max_size = pool_max_size,
            "Database connection pool created successfully"
        );

        Ok(Self { pool })
    }

    pub async fn connection(&self) -> Result<DBConnection<'_>, anyhow::Error> {
        let start = Instant::now();
        let conn = self.pool.get().await.context("Connection error")?;
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 5 {
            tracing::warn!(elapsed_ms = %elapsed.as_millis(), "pg pool_acquire slow");
        } else {
            tracing::trace!(elapsed_ms = %elapsed.as_millis(), "pg pool_acquire");
        }
        Ok(conn)
    }

    pub async fn query<T>(
        &self,
        statement: &T,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, anyhow::Error>
    where
        T: ?Sized + ToStatement,
    {
        let start = Instant::now();
        let conn = self.connection().await?;
        let rows = conn
            .query(statement, params)
            .await
            .context("Postgres error")?;
        let elapsed = start.elapsed();
        tracing::info!(row_count = rows.len(), elapsed_ms = %elapsed.as_millis(), "pg query");
        Ok(rows)
    }

    pub async fn query_one<T>(
        &self,
        statement: &T,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Row, anyhow::Error>
    where
        T: ?Sized + ToStatement,
    {
        let start = Instant::now();
        let conn = self.connection().await?;
        let row = conn
            .query_one(statement, params)
            .await
            .context("Postgres error")?;
        let elapsed = start.elapsed();
        tracing::info!(elapsed_ms = %elapsed.as_millis(), "pg query_one");
        Ok(row)
    }

    pub async fn execute<T>(
        &self,
        statement: &T,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<u64, anyhow::Error>
    where
        T: ?Sized + ToStatement,
    {
        let start = Instant::now();
        let conn = self.connection().await?;
        let rows = conn
            .execute(statement, params)
            .await
            .context("Postgres error")?;
        let elapsed = start.elapsed();
        tracing::info!(row_count = rows, elapsed_ms = %elapsed.as_millis(), "pg execute");
        Ok(rows)
    }
}

#[derive(Debug)]
pub struct ApiKey {
    pub iter: i32,
    pub salt: String,
    pub hash: String,
}

#[tracing::instrument(fields(otel.name = "pg.get_api_key"))]
pub async fn get_api_key(id: &Uuid) -> Result<Option<ApiKey>, anyhow::Error> {
    tracing::debug!(api_key_id = %id, "Looking up API key in database");

    let sql = "
        select
          key_iter,
          key_salt,
          key_hash
        from api_keys
        where api_key_id = $1
              and is_active = true
              and is_deleted = false
              and revoked_at is null
              and (expires_at is null or expires_at > now())";

    let rows = DBManager::get().await.query(sql, &[id]).await?;

    if rows.is_empty() {
        tracing::debug!(api_key_id = %id, "API key not found or inactive");
        Ok(None)
    } else {
        tracing::debug!(api_key_id = %id, "API key found");
        Ok(Some(ApiKey {
            iter: rows[0].get("key_iter"),
            salt: rows[0].get("key_salt"),
            hash: rows[0].get("key_hash"),
        }))
    }
}

#[derive(Debug)]
pub struct Vm {
    pub vm_ip: Ipv6Addr,
    pub wg_public_key: String,
    pub node_ip: IpAddr,
    pub wg_port: u16,
}

#[tracing::instrument(fields(otel.name = "pg.get_vm"))]
pub async fn get_vm(vm_id: &Uuid) -> Option<Vm> {
    let start = Instant::now();

    let sql = "select
                 vms.ip as vm_ip,
                 vms.wg_public_key,
                 vms.wg_port as wg_port,
                 nodes.ip as node_ip
               from vms
               left join nodes on vms.node_id = nodes.node_id
               where vm_id = $1
               limit 1";

    match DBManager::get().await.query(sql, &[&vm_id]).await {
        Ok(rows) => {
            let elapsed = start.elapsed();
            if rows.is_empty() {
                tracing::info!(
                    vm_id = %vm_id,
                    elapsed_ms = %elapsed.as_millis(),
                    "pg get_vm not found"
                );
                None
            } else {
                let vm = Vm {
                    vm_ip: match rows[0].get::<_, IpAddr>("vm_ip") {
                        IpAddr::V4(_ip) => panic!(
                            "This should not happen if we only insert and maintain valid IPv6 addresses"
                        ),
                        IpAddr::V6(ip) => ip,
                    },
                    wg_public_key: rows[0].get("wg_public_key"),
                    wg_port: rows[0].get::<_, i32>("wg_port") as u16,
                    node_ip: rows[0].get("node_ip"),
                };
                tracing::info!(
                    vm_id = %vm_id,
                    vm_ip = %vm.vm_ip,
                    node_ip = %vm.node_ip,
                    elapsed_ms = %elapsed.as_millis(),
                    "pg get_vm found"
                );
                Some(vm)
            }
        }
        Err(e) => {
            let elapsed = start.elapsed();
            tracing::error!(
                vm_id = %vm_id,
                elapsed_ms = %elapsed.as_millis(),
                error = ?e,
                "pg get_vm error"
            );
            None
        }
    }
}

pub struct Domain {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub vm_id: Uuid,
    pub domain: String,
    pub created_at: DateTime<Utc>,
    pub tls_cert_id: Option<Uuid>,
    pub acme_http01_challenge_domain: Option<String>,
}

pub struct TlsCert {
    pub id: Uuid,
    pub cert_chain: Vec<pem::Pem>,
    pub cert_private_key: pem::Pem,
    pub cert_not_after: DateTime<Utc>,
    pub cert_not_before: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct AcmeHttp01Challenge {
    pub domain: String,
    pub challenge_token: String,
    pub challenge_value: String,
    pub created_at: DateTime<Utc>,
}

#[tracing::instrument]
pub async fn get_cert(cert_id: Uuid) -> anyhow::Result<Option<TlsCert>> {
    tracing::debug!("Looking up");

    let sql = "select * from tls_certs where id = $1
               limit 1";

    let rows = match DBManager::get().await.query(sql, &[&cert_id]).await {
        Ok(rows) => rows,
        Err(e) => return Err(e),
    };
    if rows.is_empty() {
        tracing::debug!("cert not found");
        return Ok(None);
    }

    let cert_chain = match pem::parse_many(rows[0].get::<_, String>("cert_chain")) {
        Ok(ok) => ok,
        Err(err) => {
            tracing::error!(?err, "invalid cert chain");
            anyhow::bail!("invalid cert chain err: {err:?}");
        }
    };

    let cert_private_key = match pem::parse(rows[0].get::<_, String>("cert_private_key")) {
        Ok(ok) => ok,
        Err(err) => {
            tracing::error!(?err, "invalid private key");
            anyhow::bail!("invalid private key err: {err:?}");
        }
    };
    let cert_id: Uuid = rows[0].get("id");
    let cert_not_after = rows[0].get("cert_not_after");
    let cert_not_before = rows[0].get("cert_not_before");
    let issued_at = rows[0].get("issued_at");

    let tls = TlsCert {
        id: cert_id,
        cert_chain,
        cert_private_key,
        cert_not_after,
        cert_not_before,
        issued_at,
    };

    tracing::info!("cert found in database");
    Ok(Some(tls))
}

#[tracing::instrument(fields(otel.name = "pg.get_domain"))]
pub async fn get_domain(hostname: &str) -> anyhow::Result<Option<Domain>> {
    let hostname = normalize_domain(hostname);
    tracing::debug!(%hostname, "Looking up domain in database");

    let sql = "select * from domains where domain = $1
               limit 1";

    let rows = match DBManager::get().await.query(sql, &[&hostname]).await {
        Ok(rows) => rows,
        Err(e) => return Err(e),
    };

    let domain = if rows.is_empty() {
        tracing::debug!(
            hostname = %hostname,
            "domain not found in database"
        );
        return Ok(None);
    } else {
        Domain {
            id: rows[0].get("domain_id"),
            vm_id: rows[0].get("vm_id"),
            owner_id: rows[0].get("owner_id"),
            domain: rows[0].get("domain"),
            created_at: rows[0].get("created_at"),
            tls_cert_id: rows[0].get("tls_cert_id"),
            acme_http01_challenge_domain: rows[0].get("acme_http01_challenge_domain"),
        }
    };

    tracing::info!(
        domain_id = %&domain.id,
        owner_id = %&domain.owner_id,
        domain = %&domain.domain,
        "VM found in database"
    );
    Ok(Some(domain))
}

#[tracing::instrument]
pub async fn try_set_acme_http01_challenge(
    domain: &str,
    challenge_token: &str,
    challenge_value: &str,
) -> anyhow::Result<bool> {
    let domain = normalize_domain(domain);
    tracing::debug!(
        domain = %domain,
        challenge_token = %challenge_token,
        "Setting ACME HTTP-01 challenge in database"
    );

    let sql = "
        INSERT INTO acme_http01_challenges (domain, challenge_token, challenge_value)
        VALUES ($1, $2, $3)
        ON CONFLICT (domain) DO NOTHING RETURNING domain";

    let rows = DBManager::get()
        .await
        .execute(sql, &[&domain, &challenge_token, &challenge_value])
        .await?;

    tracing::info!(rows_affected = rows, "after statement");

    let inserted = rows == 1;

    if inserted {
        tracing::info!(
            domain = %domain,
            "ACME HTTP-01 challenge inserted successfully"
        );
        link_domain_to_challenge(&domain).await?;
    } else {
        tracing::warn!(
            domain = %domain,
            "ACME HTTP-01 challenge already exists (conflict detected)"
        );
    }

    Ok(inserted)
}

pub async fn get_acme_http01_challenge(
    domain: &str,
) -> anyhow::Result<Option<AcmeHttp01Challenge>> {
    let domain = normalize_domain(domain);
    tracing::debug!(domain = %domain, "Looking up ACME HTTP-01 challenge in database");

    let sql = "
        SELECT domain, challenge_token, challenge_value, created_at
        FROM acme_http01_challenges
        WHERE domain = $1
        LIMIT 1";

    let rows = DBManager::get().await.query(sql, &[&domain]).await?;

    if rows.is_empty() {
        tracing::debug!(domain = %domain, "ACME HTTP-01 challenge not found");
        return Ok(None);
    }

    let challenge = AcmeHttp01Challenge {
        domain: rows[0].get("domain"),
        challenge_token: rows[0].get("challenge_token"),
        challenge_value: rows[0].get("challenge_value"),
        created_at: rows[0].get("created_at"),
    };

    tracing::info!(
        domain = %domain,
        challenge_token = %challenge.challenge_token,
        "ACME HTTP-01 challenge found in database"
    );

    Ok(Some(challenge))
}

/// inserts a tls_certs row, updates foreign key on domains table
#[tracing::instrument]
pub async fn insert_cert(
    id: Uuid,
    domain: &str,
    cert_chain: &[pem::Pem],
    cert_private_key: &pem::Pem,
    cert_not_after: DateTime<Utc>,
    cert_not_before: DateTime<Utc>,
    issued_at: DateTime<Utc>,
) -> anyhow::Result<TlsCert> {
    let domain = normalize_domain(domain);
    tracing::debug!(
        cert_id = %id,
        domain = %domain,
        "Inserting TLS certificate into database"
    );

    let cert_chain_str = pem::encode_many(cert_chain);
    let cert_private_key_str = pem::encode(cert_private_key);

    let sql_insert_cert = "
        INSERT INTO tls_certs (id, cert_chain, cert_private_key, cert_not_after, cert_not_before, issued_at)
        VALUES ($1, $2, $3, $4, $5, $6)";

    DBManager::get()
        .await
        .query(
            sql_insert_cert,
            &[
                &id,
                &cert_chain_str,
                &cert_private_key_str,
                &cert_not_after,
                &cert_not_before,
                &issued_at,
            ],
        )
        .await?;

    tracing::debug!(
        cert_id = %id,
        "TLS certificate inserted, updating domain association"
    );

    let sql_update_domain = "
        UPDATE domains
        SET tls_cert_id = $1
        WHERE domain = $2
        RETURNING domain";

    let rows = DBManager::get()
        .await
        .query(sql_update_domain, &[&id, &domain])
        .await?;

    if rows.is_empty() {
        tracing::error!(
            cert_id = %id,
            domain = %domain,
            "Failed to associate certificate with domain - domain not found"
        );
        anyhow::bail!("Domain not found: {}", domain);
    }

    tracing::info!(
        cert_id = %id,
        domain = %domain,
        cert_not_after = %cert_not_after,
        cert_not_before = %cert_not_before,
        "TLS certificate inserted and associated with domain successfully"
    );

    Ok(TlsCert {
        id,
        cert_chain: cert_chain.to_vec(),
        cert_private_key: cert_private_key.clone(),
        cert_not_after,
        cert_not_before,
        issued_at,
    })
}

/// Returns (domain, tls_cert_id) pairs for domains whose cert expires within `days_before` days.
/// Excludes `exclude_cert_id` (the magic system cert that requires DNS-01 renewal).
pub async fn get_domains_needing_renewal(
    days_before: i64,
    exclude_cert_id: Uuid,
) -> anyhow::Result<Vec<(String, Uuid)>> {
    let threshold = Utc::now() + chrono::Duration::days(days_before);

    let sql = "
        SELECT d.domain, d.tls_cert_id
        FROM domains d
        JOIN tls_certs tc ON d.tls_cert_id = tc.id
        WHERE tc.cert_not_after < $1
          AND d.tls_cert_id IS NOT NULL
          AND d.tls_cert_id != $2";

    let rows = DBManager::get()
        .await
        .query(sql, &[&threshold, &exclude_cert_id])
        .await?;

    let domains = rows
        .iter()
        .map(|row| {
            let domain: String = row.get("domain");
            let cert_id: Uuid = row.get("tls_cert_id");
            (domain, cert_id)
        })
        .collect();

    Ok(domains)
}

/// Upserts an ACME HTTP-01 challenge, replacing any existing record for the domain.
/// Used during cert renewal when a challenge record from the initial issuance already exists.
#[tracing::instrument]
pub async fn upsert_acme_http01_challenge(
    domain: &str,
    challenge_token: &str,
    challenge_value: &str,
) -> anyhow::Result<()> {
    let domain = normalize_domain(domain);

    let sql = "
        INSERT INTO acme_http01_challenges (domain, challenge_token, challenge_value)
        VALUES ($1, $2, $3)
        ON CONFLICT (domain) DO UPDATE
          SET challenge_token = EXCLUDED.challenge_token,
              challenge_value = EXCLUDED.challenge_value,
              created_at = NOW()";

    DBManager::get()
        .await
        .execute(sql, &[&domain, &challenge_token, &challenge_value])
        .await?;

    tracing::info!(%domain, "ACME HTTP-01 challenge upserted");
    link_domain_to_challenge(&domain).await?;
    Ok(())
}

/// Deletes a tls_cert row. Call after replacing a cert during renewal.
pub async fn delete_cert(cert_id: Uuid) -> anyhow::Result<()> {
    let sql = "DELETE FROM tls_certs WHERE id = $1";
    DBManager::get().await.execute(sql, &[&cert_id]).await?;
    tracing::info!(%cert_id, "old TLS cert deleted");
    Ok(())
}

pub async fn delete_acme_http01_challenge(domain: &str) -> anyhow::Result<()> {
    let domain = normalize_domain(domain);
    let sql = "DELETE FROM acme_http01_challenges WHERE domain = $1";
    let rows = DBManager::get().await.execute(sql, &[&domain]).await?;
    tracing::info!(domain = %domain, rows_deleted = rows, "ACME HTTP-01 challenge removed");
    Ok(())
}

async fn link_domain_to_challenge(domain: &str) -> anyhow::Result<()> {
    let sql = "
        UPDATE domains
        SET acme_http01_challenge_domain = $1
        WHERE domain = $1";

    let rows = DBManager::get().await.execute(sql, &[&domain]).await?;

    if rows == 0 {
        tracing::warn!(domain = %domain, "Domain row not found when linking ACME challenge");
    }

    Ok(())
}

async fn check_conn() -> Result<Option<bool>, anyhow::Error> {
    tracing::debug!("Checking database connection");
    let sql = "select 1 = 1";
    let row = DBManager::get().await.query_one(sql, &[]).await?;
    tracing::debug!("Database connection check successful");
    Ok(Some(row.is_empty()))
}

pub async fn init(connection_string: String) -> Result<(), anyhow::Error> {
    tracing::info!("Initializing database connection");

    let options = DBOptions {
        pg_params: connection_string,
        pool_max_size: 8u32,
    };

    tracing::debug!("Creating DBManager instance");
    let _ = DB_MANAGER_INSTANCE.set(DBManager::new(options).await?);

    tracing::debug!("Verifying database connectivity");
    check_conn().await.unwrap();

    tracing::info!("Database initialization complete");

    Ok(())
}
