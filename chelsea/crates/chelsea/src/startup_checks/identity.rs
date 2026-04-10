use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chelsea_db::ChelseaDb;
use tracing::{debug, info};
use util::linux::get_host_ip_addrs;
use uuid::Uuid;
use vers_pg::{
    db::VersPg,
    schema::public::tables::{RecordNode, RecordOrchestrator},
};

// Magic string that corresponds to the orchestrator crate
const VERS_REGION: &str = "us-east";

async fn get_orchestrator_in_current_region(
    vers_pg: &VersPg,
) -> Result<Option<RecordOrchestrator>> {
    vers_pg
        .public
        .orchestrators
        .fetch_by_region(VERS_REGION)
        .await
        .context("Unable to fetch orchestrator in configured region")
}

/// Wait up to 10 minutes for orchestrator record to be found in PG.
async fn wait_for_orchestrator_in_current_region(vers_pg: &VersPg) -> Result<RecordOrchestrator> {
    info!(
        VERS_REGION,
        "Waiting for orchestrator record in configured region"
    );
    tokio::time::timeout(Duration::from_mins(10), async {
        loop {
            if let Some(orchestrator) = get_orchestrator_in_current_region(vers_pg).await? {
                break Ok(orchestrator);
            }
            debug!("Orchestrator record not found, trying again in 10 seconds...");
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    })
    .await
    .context(format!(
        "Timeout while waiting for orchestrator record in configured region: {VERS_REGION}"
    ))?
}

async fn create_identity(
    vers_pg: &VersPg,
    node_id: &Uuid,
    cpu_cores_total: i32,
    memory_mib_total: i64,
    disk_size_mib_total: i64,
    network_count_total: i32,
) -> Result<RecordNode> {
    let ip = get_host_ip_addrs()?
        .into_iter()
        .next()
        .ok_or(anyhow!("Failed to determine primary IP address"))?;
    let orchestrator = wait_for_orchestrator_in_current_region(vers_pg).await?;
    let wg_private_key = orch_wg::gen_private_key();

    vers_pg
        .public
        .nodes
        .insert(
            &node_id,
            &ip,
            &orchestrator.id,
            orch_wg::gen_public_key(&wg_private_key)?.as_str(),
            &wg_private_key,
            cpu_cores_total,
            memory_mib_total,
            disk_size_mib_total,
            network_count_total,
        )
        .await
        .context("Inserting new node record into public.nodes")
}

pub async fn get_or_create_identity(
    vers_pg: &VersPg,
    cpu_cores_total: i32,
    memory_mib_total: i64,
    disk_size_mib_total: i64,
    network_count_total: i32,
) -> Result<RecordNode> {
    let chelsea_db = ChelseaDb::instance().await;
    let node_id = chelsea_db.get_or_create_node_id().await?;

    let option_node_record = vers_pg.public.nodes.fetch_by_id(&node_id).await?;
    let node_record = match option_node_record {
        Some(record) => record,
        None => {
            create_identity(
                vers_pg,
                &node_id,
                cpu_cores_total,
                memory_mib_total,
                disk_size_mib_total,
                network_count_total,
            )
            .await?
        }
    };

    Ok(node_record)
}
