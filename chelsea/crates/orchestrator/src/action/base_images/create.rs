use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::action::{Action, ActionContext, ActionError, ChooseNode, RechooseNodeError};
use crate::db::{
    ApiKeyEntity, BaseImageInsertError, BaseImageJobsRepository, BaseImagesRepository,
    ChelseaNodeRepository, DBError, ImageSource, generate_rbd_image_name,
};
use crate::outbound::node_proto::HttpError;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateBaseImageRequest {
    pub image_name: String,
    pub source: ImageSourceRequest,
    /// Additional capacity in MiB beyond the actual filesystem size (defaults to 256).
    /// The final image size = calculated rootfs size + this value.
    /// Set to 0 for minimum possible image size, or higher for more free space.
    #[serde(default = "default_additional_capacity")]
    pub size_mib: i32,
    pub description: Option<String>,
}

fn default_additional_capacity() -> i32 {
    256
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSourceRequest {
    Docker { image_ref: String },
    S3 { bucket: String, key: String },
}

impl From<ImageSourceRequest> for ImageSource {
    fn from(req: ImageSourceRequest) -> Self {
        match req {
            ImageSourceRequest::Docker { image_ref } => ImageSource::Docker { image_ref },
            ImageSourceRequest::S3 { bucket, key } => ImageSource::S3 { bucket, key },
        }
    }
}

impl From<&ImageSourceRequest> for ImageSource {
    fn from(req: &ImageSourceRequest) -> Self {
        match req {
            ImageSourceRequest::Docker { image_ref } => ImageSource::Docker {
                image_ref: image_ref.clone(),
            },
            ImageSourceRequest::S3 { bucket, key } => ImageSource::S3 {
                bucket: bucket.clone(),
                key: key.clone(),
            },
        }
    }
}

pub struct CreateBaseImage {
    key: ApiKeyEntity,
    request: CreateBaseImageRequest,
    request_id: Option<String>,
}

impl CreateBaseImage {
    pub fn new(key: ApiKeyEntity, request: CreateBaseImageRequest) -> Self {
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

/// Minimum additional capacity (0 = use only the calculated rootfs size)
const MIN_ADDITIONAL_CAPACITY_MIB: i32 = 0;
/// Maximum additional capacity (32 GiB)
const MAX_ADDITIONAL_CAPACITY_MIB: i32 = 32 * 1024;

#[derive(Debug, Error)]
pub enum CreateBaseImageError {
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
    #[error("failed to create image on node: {0}")]
    NodeError(#[from] HttpError),
    #[error("internal server error")]
    InternalServerError,
}

impl From<BaseImageInsertError> for CreateBaseImageError {
    fn from(err: BaseImageInsertError) -> Self {
        match err {
            BaseImageInsertError::ImageNameExists(name) => {
                CreateBaseImageError::ImageNameExists(name)
            }
            BaseImageInsertError::DBError(e) => CreateBaseImageError::Db(e),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateBaseImageResponse {
    pub job_id: String,
    pub image_name: String,
    pub status: String,
}

impl Action for CreateBaseImage {
    type Response = CreateBaseImageResponse;
    type Error = CreateBaseImageError;
    const ACTION_ID: &'static str = "base_images.create";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        // Validate image name
        if self.request.image_name.is_empty() || self.request.image_name.len() > 128 {
            return Err(CreateBaseImageError::InvalidImageName);
        }

        // Validate additional capacity
        if self.request.size_mib < MIN_ADDITIONAL_CAPACITY_MIB
            || self.request.size_mib > MAX_ADDITIONAL_CAPACITY_MIB
        {
            return Err(CreateBaseImageError::InvalidAdditionalCapacity);
        }

        let owner_id = self.key.id();

        // Check if image name already exists for this owner (completed images)
        if ctx
            .db
            .base_images()
            .exists_by_owner_and_name(owner_id, &self.request.image_name)
            .await?
        {
            return Err(CreateBaseImageError::ImageNameExists(
                self.request.image_name.clone(),
            ));
        }

        // Also check if there's already a pending job for this image name
        if ctx
            .db
            .base_image_jobs()
            .has_pending_job_for_name(owner_id, &self.request.image_name)
            .await?
        {
            return Err(CreateBaseImageError::ImageNameExists(
                self.request.image_name.clone(),
            ));
        }

        // Generate the RBD image name in owner_id/image_name format (uses RBD namespaces)
        let rbd_image_name = generate_rbd_image_name(owner_id, &self.request.image_name);

        // Convert API request source to strongly-typed ImageSource
        let source: ImageSource = (&self.request.source).into();

        // Create a job to track the image creation
        let job = ctx
            .db
            .base_image_jobs()
            .insert(
                &self.request.image_name,
                &rbd_image_name,
                owner_id,
                &source,
                self.request.size_mib,
            )
            .await?;

        tracing::info!(
            job_id = %job.job_id,
            image_name = %self.request.image_name,
            rbd_image_name = %rbd_image_name,
            owner_id = %owner_id,
            "Created base image creation job"
        );

        // Select a node to handle the image creation
        let node_id = match crate::action::call(ChooseNode::new()).await {
            Ok(mut candidates) => match candidates.next_node() {
                Some(r) => r.node_id(),
                None => return Err(CreateBaseImageError::NoNodes(RechooseNodeError::NoNodes)),
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
                    ActionError::Error(e) => Err(CreateBaseImageError::NoNodes(e)),
                    ActionError::Panic | ActionError::Timeout | ActionError::Shutdown => {
                        Err(CreateBaseImageError::InternalServerError)
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
                return Err(CreateBaseImageError::InternalServerError);
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
            "Assigned node for base image creation"
        );

        // Convert our source format to Chelsea's format
        let chelsea_source = match &self.request.source {
            ImageSourceRequest::Docker { image_ref } => {
                dto_lib::chelsea_server2::images::ImageSource::Docker {
                    image_ref: image_ref.clone(),
                }
            }
            ImageSourceRequest::S3 { bucket, key } => {
                dto_lib::chelsea_server2::images::ImageSource::S3 {
                    bucket: bucket.clone(),
                    key: key.clone(),
                }
            }
        };

        // Build the request for Chelsea - use the RBD image name (owner_id/image_name format)
        // which places the image in the owner's RBD namespace, allowing different owners to
        // have images with the same user-facing name
        let chelsea_request = dto_lib::chelsea_server2::images::CreateBaseImageRequest {
            image_name: rbd_image_name.clone(),
            source: chelsea_source,
            size_mib: self.request.size_mib as u32,
        };

        // Call Chelsea to start image creation
        match ctx
            .proto()
            .create_image(&node, chelsea_request, self.request_id.as_deref())
            .await
        {
            Ok(response) => {
                tracing::info!(
                    job_id = %job.job_id,
                    node_id = %node_id,
                    chelsea_status = ?response.status,
                    "Successfully initiated image creation on Chelsea node"
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
                    "Failed to initiate image creation on Chelsea node"
                );

                // Mark the job as failed
                ctx.db
                    .base_image_jobs()
                    .mark_failed(job.job_id, &format!("Failed to initiate on node: {}", e))
                    .await?;

                return Err(CreateBaseImageError::NodeError(e));
            }
        }

        Ok(CreateBaseImageResponse {
            job_id: job.job_id.to_string(),
            image_name: self.request.image_name,
            status: "creating".to_string(),
        })
    }
}

pub struct GetBaseImageStatus {
    key: ApiKeyEntity,
    image_name: String,
}

impl GetBaseImageStatus {
    pub fn new(key: ApiKeyEntity, image_name: String) -> Self {
        Self { key, image_name }
    }
}

#[derive(Debug, Error)]
pub enum GetBaseImageStatusError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("image not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BaseImageStatusResponse {
    pub image_name: String,
    pub status: String,
    pub size_mib: i32,
    pub error_message: Option<String>,
}

impl Action for GetBaseImageStatus {
    type Response = BaseImageStatusResponse;
    type Error = GetBaseImageStatusError;
    const ACTION_ID: &'static str = "base_images.get_status";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        let owner_id = self.key.id();

        // First check if there's a completed image
        if let Some(image) = ctx
            .db
            .base_images()
            .get_by_owner_and_name(owner_id, &self.image_name)
            .await?
        {
            return Ok(BaseImageStatusResponse {
                image_name: image.image_name,
                status: "completed".to_string(),
                size_mib: image.size_mib,
                error_message: None,
            });
        }

        // Check if there's a job in progress
        let jobs = ctx.db.base_image_jobs().list_by_owner(owner_id).await?;
        if let Some(job) = jobs.iter().find(|j| j.image_name == self.image_name) {
            return Ok(BaseImageStatusResponse {
                image_name: job.image_name.clone(),
                status: job.status.clone(),
                size_mib: job.size_mib,
                error_message: job.error_message.clone(),
            });
        }

        // Also check public images
        let public_images = ctx.db.base_images().list_public().await?;
        if let Some(image) = public_images
            .iter()
            .find(|i| i.image_name == self.image_name)
        {
            return Ok(BaseImageStatusResponse {
                image_name: image.image_name.clone(),
                status: "completed".to_string(),
                size_mib: image.size_mib,
                error_message: None,
            });
        }

        Err(GetBaseImageStatusError::NotFound)
    }
}

impl_error_response!(CreateBaseImageError,
    CreateBaseImageError::Db(_) => INTERNAL_SERVER_ERROR,
    CreateBaseImageError::ImageNameExists(_) => CONFLICT,
    CreateBaseImageError::Forbidden => FORBIDDEN,
    CreateBaseImageError::InvalidImageName => BAD_REQUEST,
    CreateBaseImageError::InvalidAdditionalCapacity => BAD_REQUEST,
    CreateBaseImageError::NoNodes(_) => INTERNAL_SERVER_ERROR,
    CreateBaseImageError::NodeError(_) => INTERNAL_SERVER_ERROR,
    CreateBaseImageError::InternalServerError => INTERNAL_SERVER_ERROR,
);

impl_error_response!(GetBaseImageStatusError,
    GetBaseImageStatusError::Db(_) => INTERNAL_SERVER_ERROR,
    GetBaseImageStatusError::NotFound => NOT_FOUND,
    GetBaseImageStatusError::Forbidden => FORBIDDEN,
);
