use anyhow::Context;
use uuid::Uuid;

use crate::{
    action,
    db::{ChelseaNodeRepository, DB, HealthCheckRepository, HealthCheckTelemetry, NodeStatus},
    outbound::node_proto::ChelseaProto,
};

/// Run a single health-check pass across every node under this orchestrator.
///
/// For each node: pings it, records status + telemetry in the DB, and clears
/// stale pending allocations when a node is confirmed Up.
pub async fn check_all_nodes(db: &DB, proto: &ChelseaProto, orch_id: &Uuid) -> anyhow::Result<()> {
    let nodes = db
        .node()
        .all_under_orchestrator(orch_id)
        .await
        .context("failed to list nodes")?;

    if nodes.is_empty() {
        tracing::warn!("healthcheck: no nodes registered");
    }

    tracing::info!(node_count = nodes.len(), "Health check cycle starting");

    let mut up_count = 0u32;
    let mut down_count = 0u32;

    for node in &nodes {
        let passed = match proto.system_health(node, None).await {
            Ok(()) => true,
            Err(err) => {
                tracing::debug!(
                    node_id = %node.id(),
                    node_ip = %node.ip_pub(),
                    error = ?err,
                    "Health check probe failed"
                );
                false
            }
        };

        let old_statuses = db
            .health()
            .last_5(node.id())
            .await
            .context("failed to fetch health history")?;

        let last_status = old_statuses
            .first()
            .map(|v| *v.status())
            .unwrap_or(NodeStatus::Unknown);

        let new_status = if passed {
            NodeStatus::Up
        } else {
            NodeStatus::Down
        };

        // Fetch telemetry when node is healthy to cache resource availability
        let telemetry = if passed {
            match proto.system_telemetry(node, None).await {
                Ok(telem) => {
                    let t = HealthCheckTelemetry {
                        vcpu_available: Some(telem.cpu.vcpu_count_vm_available as i32),
                        mem_mib_available: Some(telem.ram.vm_mib_available as i64),
                    };

                    // Keep hardware totals in sync with telemetry on every cycle.
                    // Handles initial 0s from AddNode, node resizing, or bad inserts.
                    let reported_cpu = telem.cpu.vcpu_count_total as i32;
                    let reported_mem = telem.ram.real_mib_total as i64;
                    if reported_cpu > 0 && reported_mem > 0 {
                        if let Err(err) = db
                            .node()
                            .update_resources(node.id(), reported_cpu, reported_mem)
                            .await
                        {
                            tracing::warn!(
                                node_id = %node.id(),
                                error = ?err,
                                "Failed to update node hardware totals from telemetry"
                            );
                        }
                    }

                    tracing::debug!(
                        node_id = %node.id(),
                        node_ip = %node.ip_pub(),
                        vcpu_total = reported_cpu,
                        vcpu_available = telem.cpu.vcpu_count_vm_available,
                        mem_total_mib = reported_mem,
                        mem_available_mib = telem.ram.vm_mib_available,
                        status = "up",
                        prev_status = ?last_status,
                        "Health check + telemetry"
                    );
                    Some(t)
                }
                Err(err) => {
                    tracing::warn!(
                        node_id = %node.id(),
                        error = ?err,
                        "Failed to fetch telemetry for healthy node"
                    );
                    None
                }
            }
        } else {
            tracing::debug!(
                node_id = %node.id(),
                node_ip = %node.ip_pub(),
                status = "down",
                prev_status = ?last_status,
                "Health check result"
            );
            None
        };

        // Always insert when node is Up (to record fresh telemetry) or when status changes
        let will_write =
            new_status != last_status || (new_status == NodeStatus::Up && telemetry.is_some());
        if will_write {
            tracing::debug!(
                node_id = %node.id(),
                new_status = ?new_status,
                prev_status = ?last_status,
                has_telemetry = telemetry.is_some(),
                "Writing health check to DB"
            );
            let _ = db.health().insert(*node.id(), new_status, telemetry).await;
        }

        // When fresh telemetry arrives, clear pending allocations for this node.
        // The telemetry now reflects actual resource usage (including committed VMs),
        // so any pending reservations are stale and would cause double-counting.
        if new_status == NodeStatus::Up {
            action::clear_pending_for_node(node.id());
            up_count += 1;
        } else {
            down_count += 1;
        }

        // Log status changes worth noting
        match new_status {
            NodeStatus::Down => {
                tracing::warn!(node_id = ?node.id(), "noting node down");
            }
            NodeStatus::Unknown => {
                tracing::warn!(node_id = ?node.id(), "noting unknown status");
            }
            _ => {}
        }
    }

    tracing::info!(
        total = nodes.len(),
        up = up_count,
        down = down_count,
        "Health check cycle complete"
    );

    Ok(())
}
