mod health;

use std::time::Duration;

use tokio::{
    sync::oneshot::{self, Receiver},
    time,
};
use vers_config::VersConfig;

use crate::{
    action::{self, ActionError, PollBaseImageStatus, ReconcileGhostVms},
    tokio_util::{GracefulShutdown, TokioTaskGracefulShutdown},
};

pub struct BG;

impl BG {
    pub fn run() -> GracefulShutdown {
        let (heart_sender, heart_receiver) = oneshot::channel();
        let (image_sender, image_receiver) = oneshot::channel();
        let (reconcile_sender, reconcile_receiver) = oneshot::channel();

        let health_shutdown = TokioTaskGracefulShutdown {
            task: tokio::spawn(Self::run_healthchecks_and_rechoose(heart_receiver)),
            sender: heart_sender,
            label: Some("health"),
        };

        let image_poll_shutdown = TokioTaskGracefulShutdown {
            task: tokio::spawn(Self::run_base_image_status_poll(image_receiver)),
            sender: image_sender,
            label: Some("base_image_poll"),
        };

        let reconcile_shutdown = TokioTaskGracefulShutdown {
            task: tokio::spawn(Self::run_vm_reconciliation(reconcile_receiver)),
            sender: reconcile_sender,
            label: Some("vm_reconciliation"),
        };

        GracefulShutdown(vec![
            health_shutdown,
            image_poll_shutdown,
            reconcile_shutdown,
        ])
    }
    async fn run_healthchecks_and_rechoose(mut shutdown_receiver: Receiver<()>) {
        let ctx = action::context();

        loop {
            tokio::select! {
                _ = &mut shutdown_receiver => break,
                _  = time::sleep(Duration::from_secs(5)) => {}
            };

            if let Err(err) = health::check_all_nodes(&ctx.db, ctx.proto(), ctx.orch.id()).await {
                tracing::error!(?err, "health check failed");
            }
        }
    }

    /// Background task to poll Chelsea nodes for base image creation status
    async fn run_base_image_status_poll(mut shutdown_receiver: Receiver<()>) {
        // Poll every 10 seconds for image creation status
        const POLL_INTERVAL: Duration = Duration::from_secs(10);

        loop {
            tokio::select! {
                _ = &mut shutdown_receiver => break,
                _ = time::sleep(POLL_INTERVAL) => {}
            };

            match action::call(PollBaseImageStatus::new()).await {
                Ok(results) => {
                    // Results are logged inside the action
                    let _ = results;
                }
                Err(err) => match err {
                    ActionError::Timeout => {
                        tracing::warn!("Base image status poll timed out");
                    }
                    ActionError::Panic => {
                        tracing::error!("Base image status poll panicked");
                    }
                    ActionError::Error(err) => {
                        tracing::error!(?err, "Base image status poll error");
                    }
                    ActionError::Shutdown => break,
                },
            }
        }
    }

    /// Background task that periodically reconciles orchestrator VM records against
    /// Chelsea nodes. Any VM that exists in the orch DB but returns 404 from Chelsea
    /// is soft-deleted. This is the safety net for ghost VMs from any source.
    async fn run_vm_reconciliation(mut shutdown_receiver: Receiver<()>) {
        let interval =
            Duration::from_secs(VersConfig::orchestrator().vm_reconciliation_interval_secs);

        loop {
            tokio::select! {
                _ = &mut shutdown_receiver => break,
                _ = time::sleep(interval) => {}
            };

            match action::call(ReconcileGhostVms::new()).await {
                Ok(result) => {
                    if !result.ghost_vms_deleted.is_empty() {
                        tracing::info!(
                            count = result.ghost_vms_deleted.len(),
                            "VM reconciliation cleaned up ghost VMs"
                        );
                    }
                    if !result.errors.is_empty() {
                        tracing::warn!(
                            count = result.errors.len(),
                            "VM reconciliation encountered errors for some VMs"
                        );
                    }
                }
                Err(err) => match err {
                    ActionError::Timeout => {
                        tracing::warn!("VM reconciliation timed out");
                    }
                    ActionError::Panic => {
                        tracing::error!("VM reconciliation panicked");
                    }
                    ActionError::Error(err) => {
                        tracing::error!(?err, "VM reconciliation error");
                    }
                    ActionError::Shutdown => break,
                },
            }
        }
    }
}
