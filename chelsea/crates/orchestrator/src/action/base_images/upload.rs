use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::action::{Action, ActionContext, ActionError, ChooseNode, RechooseNodeError};
use crate::db::{
    ApiKeyEntity, BaseImageJobsRepository, BaseImagesRepository, ChelseaNodeRepository, DBError,
    ImageSource, generate_rbd_image_name,
};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone)]
pub struct UploadBaseImageRequest {
    pub image_name: String,
    /// Additional capacity in MiB beyond the actual filesystem size.
    pub size_mib: i32,
    pub tarball_path: PathBuf,
    pub tarball_size: u64,
}

/// Minimum additional capacity (0 = use only the calculated rootfs size)
const MIN_ADDITIONAL_CAPACITY_MIB: i32 = 0;
/// Maximum additional capacity (32 GiB)
const MAX_ADDITIONAL_CAPACITY_MIB: i32 = 32 * 1024;

pub struct UploadBaseImage {
    key: ApiKeyEntity,
    request: UploadBaseImageRequest,
    request_id: Option<String>,
}

impl UploadBaseImage {
    pub fn new(key: ApiKeyEntity, request: UploadBaseImageRequest) -> Self {
        Self {
            key,
            request,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum UploadBaseImageError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("image name already exists: {0}")]
    ImageNameExists(String),
    #[error("forbidden")]
    Forbidden,
    #[error("invalid image name")]
    InvalidImageName,
    #[error(
        "invalid additional capacity: must be between {MIN_ADDITIONAL_CAPACITY_MIB} MiB and {MAX_ADDITIONAL_CAPACITY_MIB} MiB"
    )]
    InvalidAdditionalCapacity,
    #[error("no available nodes")]
    NoNodes(#[from] RechooseNodeError),
    #[error("failed to upload to node: {0}")]
    NodeError(#[from] HttpError),
    #[error("internal server error")]
    InternalServerError,
    #[error("tarball not found: {0}")]
    TarballNotFound(String),
    #[error("io error: {0}")]
    IoError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UploadBaseImageResponse {
    pub job_id: String,
    pub image_name: String,
    pub status: String,
}

impl Action for UploadBaseImage {
    type Response = UploadBaseImageResponse;
    type Error = UploadBaseImageError;
    const ACTION_ID: &'static str = "base_images.upload";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        // Validate image name
        if self.request.image_name.is_empty() || self.request.image_name.len() > 128 {
            return Err(UploadBaseImageError::InvalidImageName);
        }

        // Validate additional capacity
        if self.request.size_mib < MIN_ADDITIONAL_CAPACITY_MIB
            || self.request.size_mib > MAX_ADDITIONAL_CAPACITY_MIB
        {
            return Err(UploadBaseImageError::InvalidAdditionalCapacity);
        }

        // Check tarball exists
        if !self.request.tarball_path.exists() {
            return Err(UploadBaseImageError::TarballNotFound(
                self.request.tarball_path.display().to_string(),
            ));
        }

        let owner_id = self.key.id();

        // Check if image name already exists for this owner
        if ctx
            .db
            .base_images()
            .exists_by_owner_and_name(owner_id, &self.request.image_name)
            .await?
        {
            return Err(UploadBaseImageError::ImageNameExists(
                self.request.image_name.clone(),
            ));
        }

        // Generate the RBD image name in owner_id/image_name format (uses RBD namespaces)
        let rbd_image_name = generate_rbd_image_name(owner_id, &self.request.image_name);

        // Create a job to track the image creation
        let job = ctx
            .db
            .base_image_jobs()
            .insert(
                &self.request.image_name,
                &rbd_image_name,
                owner_id,
                &ImageSource::Upload,
                self.request.size_mib,
            )
            .await?;

        tracing::info!(
            job_id = %job.job_id,
            image_name = %self.request.image_name,
            rbd_image_name = %rbd_image_name,
            owner_id = %owner_id,
            "Created base image upload job"
        );

        // Select a node to handle the image creation
        let node_id = match crate::action::call(ChooseNode::new()).await {
            Ok(mut candidates) => match candidates.next_node() {
                Some(r) => r.node_id(),
                None => return Err(UploadBaseImageError::NoNodes(RechooseNodeError::NoNodes)),
            },
            Err(err) => {
                let error_msg = match &err {
                    ActionError::Error(e) => format!("No available nodes: {}", e),
                    ActionError::Panic => "Internal error: panic during node selection".to_string(),
                    ActionError::Timeout => {
                        "Internal error: timeout during node selection".to_string()
                    }
                    ActionError::Shutdown => {
                        "Internal error: shutdown during node selection".to_string()
                    }
                };

                // Mark job as failed since we can't proceed without a node
                ctx.db
                    .base_image_jobs()
                    .mark_failed(job.job_id, &error_msg)
                    .await?;

                return match err {
                    ActionError::Error(e) => Err(UploadBaseImageError::NoNodes(e)),
                    ActionError::Panic | ActionError::Timeout | ActionError::Shutdown => {
                        Err(UploadBaseImageError::InternalServerError)
                    }
                };
            }
        };

        let node = match ctx.db.node().get_by_id(&node_id).await? {
            Some(n) => n,
            None => {
                // Node was deleted between selection and lookup - mark job as failed
                tracing::error!(
                    job_id = %job.job_id,
                    node_id = %node_id,
                    "Selected node no longer exists"
                );
                ctx.db
                    .base_image_jobs()
                    .mark_failed(job.job_id, "Selected node no longer exists")
                    .await?;
                return Err(UploadBaseImageError::InternalServerError);
            }
        };

        // Assign the node to the job
        ctx.db
            .base_image_jobs()
            .assign_node(job.job_id, node_id)
            .await?;

        tracing::info!(
            job_id = %job.job_id,
            node_id = %node_id,
            "Assigned node for base image upload"
        );

        // Open the tarball and create a stream
        let file = tokio::fs::File::open(&self.request.tarball_path)
            .await
            .map_err(|e| UploadBaseImageError::IoError(e.to_string()))?;

        let stream = tokio_util::io::ReaderStream::new(file);
        let body = reqwest::Body::wrap_stream(stream);

        // Upload to Chelsea - use the RBD image name (not the user-facing name)
        match ctx
            .proto()
            .upload_image(
                &node,
                &rbd_image_name,
                Some(self.request.size_mib as u32),
                body,
                self.request.tarball_size,
                self.request_id.as_deref(),
            )
            .await
        {
            Ok(response) => {
                tracing::info!(
                    job_id = %job.job_id,
                    node_id = %node_id,
                    chelsea_status = ?response.status,
                    "Successfully initiated upload on Chelsea node"
                );

                // Update job status to indicate it's being processed
                ctx.db
                    .base_image_jobs()
                    .update_status(job.job_id, "creating", None)
                    .await?;
            }
            Err(e) => {
                tracing::error!(
                    job_id = %job.job_id,
                    node_id = %node_id,
                    error = ?e,
                    "Failed to upload to Chelsea node"
                );

                // Mark the job as failed
                ctx.db
                    .base_image_jobs()
                    .mark_failed(job.job_id, &format!("Failed to upload to node: {}", e))
                    .await?;

                return Err(UploadBaseImageError::NodeError(e));
            }
        }

        Ok(UploadBaseImageResponse {
            job_id: job.job_id.to_string(),
            image_name: self.request.image_name,
            status: "creating".to_string(),
        })
    }
}

impl_error_response!(UploadBaseImageError,
    UploadBaseImageError::Db(_) => INTERNAL_SERVER_ERROR,
    UploadBaseImageError::ImageNameExists(_) => CONFLICT,
    UploadBaseImageError::Forbidden => FORBIDDEN,
    UploadBaseImageError::InvalidImageName => BAD_REQUEST,
    UploadBaseImageError::InvalidAdditionalCapacity => BAD_REQUEST,
    UploadBaseImageError::NoNodes(_) => INTERNAL_SERVER_ERROR,
    UploadBaseImageError::NodeError(_) => INTERNAL_SERVER_ERROR,
    UploadBaseImageError::InternalServerError => INTERNAL_SERVER_ERROR,
    UploadBaseImageError::TarballNotFound(_) => INTERNAL_SERVER_ERROR,
    UploadBaseImageError::IoError(_) => INTERNAL_SERVER_ERROR,
);
