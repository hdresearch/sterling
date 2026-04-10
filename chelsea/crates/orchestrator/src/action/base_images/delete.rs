use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::action::{Action, ActionContext, ActionError, ChooseNode, RechooseNodeError};
use crate::db::{ApiKeyEntity, BaseImagesRepository, ChelseaNodeRepository, DBError};
use crate::outbound::node_proto::HttpError;

pub struct DeleteBaseImage {
    key: ApiKeyEntity,
    base_image_id: Uuid,
    request_id: Option<String>,
}

impl DeleteBaseImage {
    pub fn new(key: ApiKeyEntity, base_image_id: Uuid) -> Self {
        Self {
            key,
            base_image_id,
            request_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }
}

#[derive(Debug, Error)]
pub enum DeleteBaseImageError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("image not found")]
    ImageNotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("no available nodes")]
    NoNodes(#[from] RechooseNodeError),
    #[error("failed to delete image on node: {0}")]
    NodeError(#[from] HttpError),
    #[error("image is in use by VMs")]
    ImageInUse(String),
    #[error("internal server error")]
    InternalServerError,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeleteBaseImageResponse {
    pub deleted: bool,
    pub base_image_id: String,
}

impl Action for DeleteBaseImage {
    type Response = DeleteBaseImageResponse;
    type Error = DeleteBaseImageError;
    const ACTION_ID: &'static str = "base_images.delete";

    async fn call(self, ctx: &ActionContext) -> Result<Self::Response, Self::Error> {
        let owner_id = self.key.id();

        // Look up the image
        let image = ctx
            .db
            .base_images()
            .get_by_id(self.base_image_id)
            .await?
            .ok_or(DeleteBaseImageError::ImageNotFound)?;

        // Verify ownership
        if image.owner_id != owner_id {
            return Err(DeleteBaseImageError::Forbidden);
        }

        // Select a node to perform the deletion
        // Since Ceph is shared storage, any node can delete the image
        let node_id = match crate::action::call(ChooseNode::new()).await {
            Ok(mut candidates) => match candidates.next_node() {
                Some(r) => r.node_id(),
                None => return Err(DeleteBaseImageError::NoNodes(RechooseNodeError::NoNodes)),
            },
            Err(err) => {
                return match err {
                    ActionError::Error(e) => Err(DeleteBaseImageError::NoNodes(e)),
                    ActionError::Panic | ActionError::Timeout | ActionError::Shutdown => {
                        Err(DeleteBaseImageError::InternalServerError)
                    }
                };
            }
        };

        let node = match ctx.db.node().get_by_id(&node_id).await? {
            Some(n) => n,
            None => {
                tracing::error!(node_id = %node_id, "Selected node no longer exists");
                return Err(DeleteBaseImageError::InternalServerError);
            }
        };

        tracing::info!(
            base_image_id = %self.base_image_id,
            image_name = %image.image_name,
            rbd_image_name = %image.rbd_image_name,
            node_id = %node_id,
            "Attempting to delete base image"
        );

        // Call Chelsea to delete the image from Ceph
        match ctx
            .proto()
            .delete_image(&node, &image.rbd_image_name, self.request_id.as_deref())
            .await
        {
            Ok(()) => {
                tracing::info!(
                    base_image_id = %self.base_image_id,
                    "Successfully deleted image from Ceph"
                );
            }
            Err(HttpError::NonSuccessStatusCode(404, _)) => {
                // Image doesn't exist in Ceph - that's fine, we'll still delete from DB
                tracing::warn!(
                    base_image_id = %self.base_image_id,
                    rbd_image_name = %image.rbd_image_name,
                    "Image not found in Ceph, proceeding with database deletion"
                );
            }
            Err(HttpError::NonSuccessStatusCode(409, msg)) => {
                // Image is in use (has child clones)
                tracing::warn!(
                    base_image_id = %self.base_image_id,
                    error = %msg,
                    "Cannot delete image: it has child clones"
                );
                return Err(DeleteBaseImageError::ImageInUse(msg));
            }
            Err(e) => {
                tracing::error!(
                    base_image_id = %self.base_image_id,
                    error = %e,
                    "Failed to delete image from Ceph"
                );
                return Err(DeleteBaseImageError::NodeError(e));
            }
        }

        // Delete from database
        let deleted = ctx
            .db
            .base_images()
            .delete(self.base_image_id, owner_id)
            .await?;

        if deleted {
            tracing::info!(
                base_image_id = %self.base_image_id,
                "Successfully deleted base image from database"
            );
        } else {
            tracing::warn!(
                base_image_id = %self.base_image_id,
                "Image was not found in database during deletion"
            );
        }

        Ok(DeleteBaseImageResponse {
            deleted,
            base_image_id: self.base_image_id.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_base_image_response_serialization() {
        let response = DeleteBaseImageResponse {
            deleted: true,
            base_image_id: "test-image-id".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"deleted\":true"));
        assert!(json.contains("\"base_image_id\":\"test-image-id\""));

        let deserialized: DeleteBaseImageResponse = serde_json::from_str(&json).unwrap();
        assert!(deserialized.deleted);
        assert_eq!(deserialized.base_image_id, "test-image-id");
    }

    #[test]
    fn test_delete_base_image_response_not_deleted() {
        let response = DeleteBaseImageResponse {
            deleted: false,
            base_image_id: "missing-image".to_string(),
        };

        assert!(!response.deleted);
        assert_eq!(response.base_image_id, "missing-image");
    }

    #[test]
    fn test_delete_base_image_response_clone() {
        let response = DeleteBaseImageResponse {
            deleted: true,
            base_image_id: "cloned-id".to_string(),
        };

        let cloned = response.clone();
        assert_eq!(cloned.deleted, response.deleted);
        assert_eq!(cloned.base_image_id, response.base_image_id);
    }

    #[test]
    fn test_delete_base_image_response_debug() {
        let response = DeleteBaseImageResponse {
            deleted: true,
            base_image_id: "debug-test".to_string(),
        };

        let debug_str = format!("{:?}", response);
        assert!(debug_str.contains("DeleteBaseImageResponse"));
        assert!(debug_str.contains("deleted: true"));
        assert!(debug_str.contains("debug-test"));
    }

    #[test]
    fn test_delete_base_image_error_display_image_not_found() {
        let err = DeleteBaseImageError::ImageNotFound;
        assert_eq!(err.to_string(), "image not found");
    }

    #[test]
    fn test_delete_base_image_error_display_forbidden() {
        let err = DeleteBaseImageError::Forbidden;
        assert_eq!(err.to_string(), "forbidden");
    }

    #[test]
    fn test_delete_base_image_error_display_image_in_use() {
        let err = DeleteBaseImageError::ImageInUse("vm-123, vm-456".to_string());
        assert_eq!(err.to_string(), "image is in use by VMs");
    }

    #[test]
    fn test_delete_base_image_error_display_internal_server_error() {
        let err = DeleteBaseImageError::InternalServerError;
        assert_eq!(err.to_string(), "internal server error");
    }

    #[test]
    fn test_delete_base_image_error_debug() {
        let err = DeleteBaseImageError::ImageNotFound;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("ImageNotFound"));

        let err = DeleteBaseImageError::ImageInUse("test-vms".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("ImageInUse"));
        assert!(debug_str.contains("test-vms"));
    }

    #[test]
    fn test_delete_base_image_action_id() {
        assert_eq!(DeleteBaseImage::ACTION_ID, "base_images.delete");
    }
}

impl_error_response!(DeleteBaseImageError,
    DeleteBaseImageError::Db(_) => INTERNAL_SERVER_ERROR,
    DeleteBaseImageError::ImageNotFound => NOT_FOUND,
    DeleteBaseImageError::Forbidden => FORBIDDEN,
    DeleteBaseImageError::NoNodes(_) => INTERNAL_SERVER_ERROR,
    DeleteBaseImageError::NodeError(_) => INTERNAL_SERVER_ERROR,
    DeleteBaseImageError::ImageInUse(_) => CONFLICT,
    DeleteBaseImageError::InternalServerError => INTERNAL_SERVER_ERROR,
);
