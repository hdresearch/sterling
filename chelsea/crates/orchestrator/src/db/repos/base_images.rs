use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::future::Future;
use thiserror::Error;
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

/// Strongly-typed image source configuration.
/// This enum serializes to a tagged JSON object (e.g., `{"type": "docker", "image_ref": "..."}`),
/// allowing the source_config JSONB to be entirely self-contained.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Docker { image_ref: String },
    S3 { bucket: String, key: String },
    Upload,
    Manual,
}

impl ImageSource {
    /// Returns the source type string for database storage.
    /// This is kept for the `source_type` column (useful for queries).
    pub fn source_type(&self) -> &'static str {
        match self {
            ImageSource::Docker { .. } => "docker",
            ImageSource::S3 { .. } => "s3",
            ImageSource::Upload => "upload",
            ImageSource::Manual => "manual",
        }
    }

    /// Construct ImageSource from legacy format (source_type string + source_config JSON).
    /// Used for backward compatibility when reading from the database.
    pub fn from_legacy(source_type: &str, source_config: &JsonValue) -> Result<Self, String> {
        match source_type {
            "docker" => {
                let image_ref = source_config
                    .get("image_ref")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing image_ref for docker source".to_string())?;
                Ok(ImageSource::Docker {
                    image_ref: image_ref.to_string(),
                })
            }
            "s3" => {
                let bucket = source_config
                    .get("bucket")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing bucket for s3 source".to_string())?;
                let key = source_config
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing key for s3 source".to_string())?;
                Ok(ImageSource::S3 {
                    bucket: bucket.to_string(),
                    key: key.to_string(),
                })
            }
            "upload" => Ok(ImageSource::Upload),
            "manual" => Ok(ImageSource::Manual),
            other => Err(format!("unknown source type: {}", other)),
        }
    }
}

/// Generate an RBD image name from owner_id and user-facing image_name.
/// Format: {owner_id}/{image_name}
///
/// This uses RBD namespaces to organize images by owner. Each owner gets their
/// own namespace (the owner_id), and the image_name is used directly within that namespace.
/// This allows different users to have images with the same name without collision.
///
/// Note: The namespace must be created in Ceph before the image can be created.
pub fn generate_rbd_image_name(owner_id: Uuid, image_name: &str) -> String {
    format!("{}/{}", owner_id, image_name)
}

/// Extract the namespace (owner_id) from an RBD image name.
/// Returns None if the format is invalid.
pub fn extract_namespace_from_rbd_name(rbd_image_name: &str) -> Option<&str> {
    rbd_image_name.split('/').next()
}

/// Repository trait for base image operations
pub trait BaseImagesRepository {
    /// Insert a new base image record
    fn insert(
        &self,
        image_name: &str,
        rbd_image_name: &str,
        owner_id: Uuid,
        is_public: bool,
        source: &ImageSource,
        size_mib: i32,
        description: Option<&str>,
    ) -> impl Future<Output = Result<BaseImageEntity, BaseImageInsertError>>;

    /// Get a base image by ID
    fn get_by_id(
        &self,
        base_image_id: Uuid,
    ) -> impl Future<Output = Result<Option<BaseImageEntity>, DBError>>;

    /// Get a base image by owner and name
    fn get_by_owner_and_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> impl Future<Output = Result<Option<BaseImageEntity>, DBError>>;

    /// Get a base image by RBD image name
    fn get_by_rbd_name(
        &self,
        rbd_image_name: &str,
    ) -> impl Future<Output = Result<Option<BaseImageEntity>, DBError>>;

    /// List all base images visible to the given owner (owned + public) with pagination
    fn list_visible_to_owner(
        &self,
        owner_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = Result<Vec<BaseImageEntity>, DBError>>;

    /// Count all base images visible to the given owner (owned + public)
    fn count_visible_to_owner(&self, owner_id: Uuid) -> impl Future<Output = Result<i64, DBError>>;

    /// List all public base images
    fn list_public(&self) -> impl Future<Output = Result<Vec<BaseImageEntity>, DBError>>;

    /// Delete a base image by ID (only if owned by the given owner)
    fn delete(
        &self,
        base_image_id: Uuid,
        owner_id: Uuid,
    ) -> impl Future<Output = Result<bool, DBError>>;

    /// Check if an image name already exists for this owner
    fn exists_by_owner_and_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> impl Future<Output = Result<bool, DBError>>;

    /// Get a base image by name that is visible to the owner (owned or public)
    fn get_visible_by_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> impl Future<Output = Result<Option<BaseImageEntity>, DBError>>;
}

/// Repository trait for base image job operations
pub trait BaseImageJobsRepository {
    /// Insert a new job
    fn insert(
        &self,
        image_name: &str,
        rbd_image_name: &str,
        owner_id: Uuid,
        source: &ImageSource,
        size_mib: i32,
    ) -> impl Future<Output = Result<BaseImageJobEntity, DBError>>;

    /// Get a job by ID
    fn get_by_id(
        &self,
        job_id: Uuid,
    ) -> impl Future<Output = Result<Option<BaseImageJobEntity>, DBError>>;

    /// Update job status
    fn update_status(
        &self,
        job_id: Uuid,
        status: &str,
        error_message: Option<&str>,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Mark job as completed
    fn mark_completed(&self, job_id: Uuid) -> impl Future<Output = Result<(), DBError>>;

    /// Mark job as failed
    fn mark_failed(
        &self,
        job_id: Uuid,
        error_message: &str,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Assign a node to a job
    fn assign_node(&self, job_id: Uuid, node_id: Uuid)
    -> impl Future<Output = Result<(), DBError>>;

    /// List jobs by owner
    fn list_by_owner(
        &self,
        owner_id: Uuid,
    ) -> impl Future<Output = Result<Vec<BaseImageJobEntity>, DBError>>;

    /// List pending jobs (for job processing)
    fn list_pending(&self) -> impl Future<Output = Result<Vec<BaseImageJobEntity>, DBError>>;

    /// Check if a pending job exists for this owner and image name
    fn has_pending_job_for_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> impl Future<Output = Result<bool, DBError>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseImageEntity {
    pub base_image_id: Uuid,
    /// User-facing image name (e.g., "ubuntu-24.04")
    pub image_name: String,
    /// Internal RBD image name in Ceph (hash of owner_id + image_name)
    pub rbd_image_name: String,
    pub owner_id: Uuid,
    pub is_public: bool,
    /// Strongly-typed source configuration
    pub source: ImageSource,
    pub size_mib: i32,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BaseImageEntity {
    fn from_row(row: &Row) -> Self {
        let source_type: String = row.get("source_type");
        let source_config: JsonValue = row.get("source_config");

        // Try to deserialize directly from source_config (new format with embedded type),
        // falling back to constructing from legacy format (separate source_type + source_config)
        let source =
            serde_json::from_value::<ImageSource>(source_config.clone()).unwrap_or_else(|_| {
                ImageSource::from_legacy(&source_type, &source_config)
                    .unwrap_or(ImageSource::Manual)
            });

        Self {
            base_image_id: row.get("base_image_id"),
            image_name: row.get("image_name"),
            rbd_image_name: row.get("rbd_image_name"),
            owner_id: row.get("owner_id"),
            is_public: row.get("is_public"),
            source,
            size_mib: row.get("size_mib"),
            description: row.get("description"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseImageJobEntity {
    pub job_id: Uuid,
    /// User-facing image name
    pub image_name: String,
    /// Internal RBD image name in Ceph
    pub rbd_image_name: String,
    pub owner_id: Uuid,
    /// Strongly-typed source configuration
    pub source: ImageSource,
    pub size_mib: i32,
    pub status: String,
    pub error_message: Option<String>,
    pub node_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl BaseImageJobEntity {
    fn from_row(row: &Row) -> Self {
        let source_type: String = row.get("source_type");
        let source_config: JsonValue = row.get("source_config");

        // Try to deserialize directly from source_config (new format with embedded type),
        // falling back to constructing from legacy format (separate source_type + source_config)
        let source =
            serde_json::from_value::<ImageSource>(source_config.clone()).unwrap_or_else(|_| {
                ImageSource::from_legacy(&source_type, &source_config)
                    .unwrap_or(ImageSource::Manual)
            });

        Self {
            job_id: row.get("job_id"),
            image_name: row.get("image_name"),
            rbd_image_name: row.get("rbd_image_name"),
            owner_id: row.get("owner_id"),
            source,
            size_mib: row.get("size_mib"),
            status: row.get("status"),
            error_message: row.get("error_message"),
            node_id: row.get("node_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            completed_at: row.get("completed_at"),
        }
    }
}

#[derive(Debug, Error)]
pub enum BaseImageInsertError {
    #[error("Image name already exists for this owner: {0}")]
    ImageNameExists(String),
    #[error("Database error: {0}")]
    DBError(#[from] DBError),
}

/// Wrapper struct for base images repository
pub struct BaseImages(DB);

impl DB {
    pub fn base_images(&self) -> BaseImages {
        BaseImages(self.clone())
    }
}

impl BaseImagesRepository for BaseImages {
    async fn insert(
        &self,
        image_name: &str,
        rbd_image_name: &str,
        owner_id: Uuid,
        is_public: bool,
        source: &ImageSource,
        size_mib: i32,
        description: Option<&str>,
    ) -> Result<BaseImageEntity, BaseImageInsertError> {
        // Serialize ImageSource to JSON (includes type tag)
        let source_config =
            serde_json::to_value(source).expect("ImageSource serialization should never fail");
        // Extract source_type string for the DB column (useful for queries)
        let source_type = source.source_type();

        let rows = query_sql!(
            self.0,
            r#"
            INSERT INTO base_images (image_name, rbd_image_name, owner_id, is_public, source_type, source_config, size_mib, description)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
            &[Type::TEXT, Type::TEXT, Type::UUID, Type::BOOL, Type::TEXT, Type::JSONB, Type::INT4, Type::TEXT],
            &[&image_name, &rbd_image_name, &owner_id, &is_public, &source_type, &source_config, &size_mib, &description]
        )
        .map_err(|e| {
            // Check for unique constraint violation
            if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
                BaseImageInsertError::ImageNameExists(image_name.to_string())
            } else {
                BaseImageInsertError::DBError(e)
            }
        })?;

        Ok(BaseImageEntity::from_row(&rows[0]))
    }

    async fn get_by_id(&self, base_image_id: Uuid) -> Result<Option<BaseImageEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM base_images WHERE base_image_id = $1",
            &[Type::UUID],
            &[&base_image_id]
        )?;

        Ok(row.map(|r| BaseImageEntity::from_row(&r)))
    }

    async fn get_by_owner_and_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> Result<Option<BaseImageEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM base_images WHERE owner_id = $1 AND image_name = $2",
            &[Type::UUID, Type::TEXT],
            &[&owner_id, &image_name]
        )?;

        Ok(row.map(|r| BaseImageEntity::from_row(&r)))
    }

    async fn get_by_rbd_name(
        &self,
        rbd_image_name: &str,
    ) -> Result<Option<BaseImageEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM base_images WHERE rbd_image_name = $1",
            &[Type::TEXT],
            &[&rbd_image_name]
        )?;

        Ok(row.map(|r| BaseImageEntity::from_row(&r)))
    }

    async fn list_visible_to_owner(
        &self,
        owner_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<BaseImageEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            r#"
            SELECT * FROM base_images
            WHERE owner_id = $1 OR is_public = TRUE
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            &[Type::UUID, Type::INT8, Type::INT8],
            &[&owner_id, &limit, &offset]
        )?;

        Ok(rows.iter().map(BaseImageEntity::from_row).collect())
    }

    async fn count_visible_to_owner(&self, owner_id: Uuid) -> Result<i64, DBError> {
        let row = query_one_sql!(
            self.0,
            r#"
            SELECT COUNT(*) as count FROM base_images
            WHERE owner_id = $1 OR is_public = TRUE
            "#,
            &[Type::UUID],
            &[&owner_id]
        )?;

        Ok(row.map(|r| r.get("count")).unwrap_or(0))
    }

    async fn list_public(&self) -> Result<Vec<BaseImageEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM base_images WHERE is_public = TRUE ORDER BY created_at DESC"
        )?;

        Ok(rows.iter().map(BaseImageEntity::from_row).collect())
    }

    async fn delete(&self, base_image_id: Uuid, owner_id: Uuid) -> Result<bool, DBError> {
        let result = execute_sql!(
            self.0,
            "DELETE FROM base_images WHERE base_image_id = $1 AND owner_id = $2",
            &[Type::UUID, Type::UUID],
            &[&base_image_id, &owner_id]
        )?;

        Ok(result > 0)
    }

    async fn exists_by_owner_and_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> Result<bool, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT EXISTS(SELECT 1 FROM base_images WHERE owner_id = $1 AND image_name = $2) as exists",
            &[Type::UUID, Type::TEXT],
            &[&owner_id, &image_name]
        )?;

        Ok(row.map(|r| r.get("exists")).unwrap_or(false))
    }

    async fn get_visible_by_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> Result<Option<BaseImageEntity>, DBError> {
        // First try to find owned image, then fall back to public image with same name
        let row = query_one_sql!(
            self.0,
            r#"
            SELECT * FROM base_images
            WHERE image_name = $2 AND (owner_id = $1 OR is_public = TRUE)
            ORDER BY (owner_id = $1) DESC
            LIMIT 1
            "#,
            &[Type::UUID, Type::TEXT],
            &[&owner_id, &image_name]
        )?;

        Ok(row.map(|r| BaseImageEntity::from_row(&r)))
    }
}

/// Wrapper struct for base image jobs repository
pub struct BaseImageJobs(DB);

impl DB {
    pub fn base_image_jobs(&self) -> BaseImageJobs {
        BaseImageJobs(self.clone())
    }
}

impl BaseImageJobsRepository for BaseImageJobs {
    async fn insert(
        &self,
        image_name: &str,
        rbd_image_name: &str,
        owner_id: Uuid,
        source: &ImageSource,
        size_mib: i32,
    ) -> Result<BaseImageJobEntity, DBError> {
        // Serialize ImageSource to JSON (includes type tag)
        let source_config =
            serde_json::to_value(source).expect("ImageSource serialization should never fail");
        // Extract source_type string for the DB column (useful for queries)
        let source_type = source.source_type();

        let rows = query_sql!(
            self.0,
            r#"
            INSERT INTO base_image_jobs (image_name, rbd_image_name, owner_id, source_type, source_config, size_mib)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
            &[
                Type::TEXT,
                Type::TEXT,
                Type::UUID,
                Type::TEXT,
                Type::JSONB,
                Type::INT4
            ],
            &[
                &image_name,
                &rbd_image_name,
                &owner_id,
                &source_type,
                &source_config,
                &size_mib
            ]
        )?;

        Ok(BaseImageJobEntity::from_row(&rows[0]))
    }

    async fn get_by_id(&self, job_id: Uuid) -> Result<Option<BaseImageJobEntity>, DBError> {
        let row = query_one_sql!(
            self.0,
            "SELECT * FROM base_image_jobs WHERE job_id = $1",
            &[Type::UUID],
            &[&job_id]
        )?;

        Ok(row.map(|r| BaseImageJobEntity::from_row(&r)))
    }

    async fn update_status(
        &self,
        job_id: Uuid,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            r#"
            UPDATE base_image_jobs
            SET status = $2, error_message = $3
            WHERE job_id = $1
            "#,
            &[Type::UUID, Type::TEXT, Type::TEXT],
            &[&job_id, &status, &error_message]
        )?;

        Ok(())
    }

    async fn mark_completed(&self, job_id: Uuid) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            r#"
            UPDATE base_image_jobs
            SET status = 'completed', completed_at = NOW()
            WHERE job_id = $1
            "#,
            &[Type::UUID],
            &[&job_id]
        )?;

        Ok(())
    }

    async fn mark_failed(&self, job_id: Uuid, error_message: &str) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            r#"
            UPDATE base_image_jobs
            SET status = 'failed', error_message = $2, completed_at = NOW()
            WHERE job_id = $1
            "#,
            &[Type::UUID, Type::TEXT],
            &[&job_id, &error_message]
        )?;

        Ok(())
    }

    async fn assign_node(&self, job_id: Uuid, node_id: Uuid) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "UPDATE base_image_jobs SET node_id = $2 WHERE job_id = $1",
            &[Type::UUID, Type::UUID],
            &[&job_id, &node_id]
        )?;

        Ok(())
    }

    async fn list_by_owner(&self, owner_id: Uuid) -> Result<Vec<BaseImageJobEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM base_image_jobs WHERE owner_id = $1 ORDER BY created_at DESC",
            &[Type::UUID],
            &[&owner_id]
        )?;

        Ok(rows.iter().map(BaseImageJobEntity::from_row).collect())
    }

    async fn list_pending(&self) -> Result<Vec<BaseImageJobEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            r#"
            SELECT * FROM base_image_jobs
            WHERE status NOT IN ('completed', 'failed')
            ORDER BY created_at ASC
            "#
        )?;

        Ok(rows.iter().map(BaseImageJobEntity::from_row).collect())
    }

    async fn has_pending_job_for_name(
        &self,
        owner_id: Uuid,
        image_name: &str,
    ) -> Result<bool, DBError> {
        let row = query_one_sql!(
            self.0,
            r#"
            SELECT EXISTS(
                SELECT 1 FROM base_image_jobs
                WHERE owner_id = $1 AND image_name = $2
                AND status NOT IN ('completed', 'failed')
            ) as exists
            "#,
            &[Type::UUID, Type::TEXT],
            &[&owner_id, &image_name]
        )?;

        Ok(row.map(|r| r.get::<_, bool>("exists")).unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_rbd_image_name() {
        let owner_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let image_name = "ubuntu-24.04";

        let rbd_name = generate_rbd_image_name(owner_id, image_name);

        // Should be in format owner_id/image_name
        assert_eq!(
            rbd_name,
            "12345678-1234-1234-1234-123456789abc/ubuntu-24.04"
        );

        // Same inputs should produce same output
        let rbd_name2 = generate_rbd_image_name(owner_id, image_name);
        assert_eq!(rbd_name, rbd_name2);

        // Different owner should produce different output
        let other_owner = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let rbd_name3 = generate_rbd_image_name(other_owner, image_name);
        assert_eq!(
            rbd_name3,
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/ubuntu-24.04"
        );
        assert_ne!(rbd_name, rbd_name3);

        // Different image name should produce different output
        let rbd_name4 = generate_rbd_image_name(owner_id, "debian-12");
        assert_eq!(rbd_name4, "12345678-1234-1234-1234-123456789abc/debian-12");
        assert_ne!(rbd_name, rbd_name4);
    }

    #[test]
    fn test_extract_namespace_from_rbd_name() {
        let rbd_name = "12345678-1234-1234-1234-123456789abc/ubuntu-24.04";
        let namespace = extract_namespace_from_rbd_name(rbd_name);
        assert_eq!(namespace, Some("12345678-1234-1234-1234-123456789abc"));

        // Old hash-style names (no slash) - returns the whole string as "namespace"
        let old_style = "a1b2c3d4e5f6g7h8";
        let namespace = extract_namespace_from_rbd_name(old_style);
        assert_eq!(namespace, Some("a1b2c3d4e5f6g7h8"));
    }
}
