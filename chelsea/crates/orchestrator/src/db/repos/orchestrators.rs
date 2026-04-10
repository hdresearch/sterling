use std::net::{IpAddr, Ipv6Addr};

use chrono::{DateTime, Utc};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait OrchestratorsRepository {
    fn insert(
        &self,
        region: &str,
        ip: IpAddr,
        wg_private_key: String,
        wg_public_key: String,
    ) -> impl Future<Output = Result<OrchestratorEntity, DBError>>;
    fn get_by_region(
        &self,
        region: &str,
    ) -> impl Future<Output = Result<Option<OrchestratorEntity>, DBError>>;
}

pub struct Orchestrator(DB);

impl DB {
    pub fn orchestrator(&self) -> Orchestrator {
        Orchestrator(self.clone())
    }
}

// unused is allowed since this represents the sql table. We might not use all
// fields.
#[allow(unused)]
#[derive(Debug, Clone)]
pub struct OrchestratorEntity {
    id: Uuid,
    // for future use
    region: String,
    ip: IpAddr,
    wg_ipv6: Ipv6Addr,
    wg_private_key: String,
    wg_public_key: String,
    created_at: DateTime<Utc>,
}

impl OrchestratorEntity {
    pub fn id(&self) -> &Uuid {
        &self.id
    }
    pub fn ip(&self) -> IpAddr {
        self.ip
    }

    pub fn wg_ipv6(&self) -> Ipv6Addr {
        self.wg_ipv6
    }

    pub fn wg_pub_key(&self) -> &str {
        self.wg_public_key.as_str()
    }

    pub fn wg_private_key(&self) -> &str {
        self.wg_private_key.as_str()
    }
}

impl From<Row> for OrchestratorEntity {
    fn from(value: Row) -> Self {
        let region = value.get::<_, String>("region");

        let wg_private_key = value.get("wg_private_key");
        let wg_public_key = value.get("wg_public_key");

        let wg_ipv6: Ipv6Addr = match value.get::<_, IpAddr>("wg_ipv6") {
            IpAddr::V4(_) => panic!(),
            IpAddr::V6(ipv6) => ipv6,
        };

        Self {
            wg_private_key,
            wg_public_key,
            wg_ipv6,
            region,
            ip: value.get("ip"),
            id: value.get("id"),
            created_at: value.get("created_at"),
        }
    }
}

impl OrchestratorsRepository for Orchestrator {
    async fn insert(
        &self,
        region: &str,
        ip: IpAddr,
        wg_private_key: String,
        wg_public_key: String,
    ) -> Result<OrchestratorEntity, DBError> {
        let created_at = Utc::now();
        let id = Uuid::new_v4();

        let maybe_row = query_one_sql!(
            self.0,
            "INSERT INTO orchestrators 
              (id, ip, region, wg_ipv6, wg_public_key, wg_private_key, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *
            ",
            &[
                Type::UUID,
                Type::INET,
                Type::TEXT,
                Type::INET,
                Type::TEXT,
                Type::TEXT,
                Type::TIMESTAMPTZ
            ],
            &[
                &id,
                &ip,
                &region,
                &"fd00:fe11:deed:0::ffff".parse::<IpAddr>().unwrap(),
                &wg_public_key,
                &wg_private_key,
                &created_at
            ]
        )?;

        let orchestrator = OrchestratorEntity::from(maybe_row.expect("RETURNING * gives row"));
        Ok(orchestrator)
    }
    async fn get_by_region(&self, region: &str) -> Result<Option<OrchestratorEntity>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT * FROM orchestrators WHERE region = $1",
            &[Type::TEXT],
            &[&region.to_string()]
        )?;

        Ok(maybe_row.map(|row| TryInto::<OrchestratorEntity>::try_into(row).unwrap()))
    }
}
