use thiserror::Error;

use crate::action::{Action, ActionContext};
use crate::db::{BaseImageJobsRepository, BaseImagesRepository, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;
use dto_lib::chelsea_server2::images::ImageCreationStatus;

pub struct PollBaseImageStatus;

impl PollBaseImageStatus {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Error)]
pub enum PollBaseImageStatusError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
}

#[derive(Debug)]
pub struct JobPollResult {
    pub job_id: uuid::Uuid,
    pub status: String,
    pub completed: bool,
    pub failed: bool,
}

impl Action for PollBaseImageStatus {
    type Response = Vec<JobPollResult>;
    type Error = PollBaseImageStatusError;
    const ACTION_ID: &'static str = "base_images.poll_status";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        // Get all pending jobs (not completed or failed)
        let pending_jobs = ctx.db.base_image_jobs().list_pending().await?;

        if pending_jobs.is_empty() {
            return Ok(Vec::new());
        }

        tracing::debug!(
            count = pending_jobs.len(),
            "Polling status for pending base image jobs"
        );

        let mut results = Vec::new();

        for job in pending_jobs {
            // Skip jobs that don't have a node assigned yet
            let Some(node_id) = job.node_id else {
                tracing::trace!(
                    job_id = %job.job_id,
                    "Skipping job without assigned node"
                );
                continue;
            };

            // Get the node
            let node = match ctx.db.node().get_by_id(&node_id).await? {
                Some(n) => n,
                None => {
                    tracing::warn!(
                        job_id = %job.job_id,
                        node_id = %node_id,
                        "Node no longer exists, marking job as failed"
                    );
                    ctx.db
                        .base_image_jobs()
                        .mark_failed(job.job_id, "Assigned node no longer exists")
                        .await?;
                    results.push(JobPollResult {
                        job_id: job.job_id,
                        status: "failed".to_string(),
                        completed: false,
                        failed: true,
                    });
                    continue;
                }
            };

            // Poll Chelsea for the image status using the RBD image name
            // Note: No request_id since this is a background poll operation
            match ctx
                .proto()
                .image_status(&node, &job.rbd_image_name, None)
                .await
            {
                Ok(status_response) => {
                    let (new_status, completed, failed) = match &status_response.status {
                        ImageCreationStatus::Completed => {
                            tracing::info!(
                                job_id = %job.job_id,
                                image_name = %job.image_name,
                                rbd_image_name = %job.rbd_image_name,
                                "Base image creation completed"
                            );

                            // Mark job as completed
                            ctx.db.base_image_jobs().mark_completed(job.job_id).await?;

                            // Create the base_images record
                            let size_mib =
                                status_response.size_mib.unwrap_or(job.size_mib as u32) as i32;
                            if let Err(e) = ctx
                                .db
                                .base_images()
                                .insert(
                                    &job.image_name,
                                    &job.rbd_image_name,
                                    job.owner_id,
                                    false, // not public by default
                                    &job.source,
                                    size_mib,
                                    None, // description
                                )
                                .await
                            {
                                tracing::error!(
                                    job_id = %job.job_id,
                                    error = ?e,
                                    "Failed to create base_images record after completion"
                                );
                            }

                            ("completed".to_string(), true, false)
                        }
                        ImageCreationStatus::Failed { error } => {
                            tracing::warn!(
                                job_id = %job.job_id,
                                image_name = %job.image_name,
                                error = %error,
                                "Base image creation failed"
                            );
                            ctx.db
                                .base_image_jobs()
                                .mark_failed(job.job_id, error)
                                .await?;
                            ("failed".to_string(), false, true)
                        }
                        status => {
                            // Update the job status to reflect current Chelsea status
                            let status_str = match status {
                                ImageCreationStatus::Pending => "pending",
                                ImageCreationStatus::Downloading => "downloading",
                                ImageCreationStatus::Extracting => "extracting",
                                ImageCreationStatus::Configuring => "configuring",
                                ImageCreationStatus::CreatingRbd => "creating_rbd",
                                ImageCreationStatus::CreatingSnapshot => "creating_snapshot",
                                _ => "creating",
                            };

                            // Only update if status changed
                            if job.status != status_str {
                                ctx.db
                                    .base_image_jobs()
                                    .update_status(job.job_id, status_str, None)
                                    .await?;
                            }

                            (status_str.to_string(), false, false)
                        }
                    };

                    results.push(JobPollResult {
                        job_id: job.job_id,
                        status: new_status,
                        completed,
                        failed,
                    });
                }
                Err(HttpError::NonSuccessStatusCode(404, _)) => {
                    // Image not found on Chelsea - it might not have started yet
                    // or was cleaned up.
                    //
                    // Don't immediately mark as failed - give Chelsea time to process.
                    // The image creation takes 10-30+ seconds, and during this time
                    // Chelsea may return 404 until the process completes.
                    //
                    // Only mark as failed if the job has been pending for too long
                    // (more than 2 minutes since creation).
                    let age_seconds = (chrono::Utc::now() - job.created_at).num_seconds();
                    const GRACE_PERIOD_SECONDS: i64 = 120;

                    if age_seconds > GRACE_PERIOD_SECONDS {
                        tracing::warn!(
                            job_id = %job.job_id,
                            image_name = %job.image_name,
                            rbd_image_name = %job.rbd_image_name,
                            age_seconds = age_seconds,
                            "Image not found on Chelsea node after grace period, marking as failed"
                        );
                        ctx.db
                            .base_image_jobs()
                            .mark_failed(job.job_id, "Image not found on Chelsea node")
                            .await?;
                        results.push(JobPollResult {
                            job_id: job.job_id,
                            status: "failed".to_string(),
                            completed: false,
                            failed: true,
                        });
                    } else {
                        tracing::debug!(
                            job_id = %job.job_id,
                            image_name = %job.image_name,
                            age_seconds = age_seconds,
                            "Image not found on Chelsea yet, still within grace period"
                        );
                    }
                }
                Err(e) => {
                    // Log but don't fail the whole poll operation
                    tracing::warn!(
                        job_id = %job.job_id,
                        node_id = %node_id,
                        error = ?e,
                        "Failed to poll image status from Chelsea node"
                    );
                }
            }
        }

        if !results.is_empty() {
            let completed_count = results.iter().filter(|r| r.completed).count();
            let failed_count = results.iter().filter(|r| r.failed).count();
            tracing::info!(
                total = results.len(),
                completed = completed_count,
                failed = failed_count,
                "Base image status poll complete"
            );
        }

        Ok(results)
    }
}
