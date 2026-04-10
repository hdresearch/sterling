use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Request body for POST /api/v1/commit_tags
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct CreateTagRequest {
    /// The name of the tag (alphanumeric, hyphens, underscores, dots, 1-64 chars)
    pub tag_name: String,
    /// The commit ID this tag should point to
    pub commit_id: Uuid,
    /// Optional description of what this tag represents
    pub description: Option<String>,
}

/// Response body for POST /api/v1/commit_tags
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct CreateTagResponse {
    /// The ID of the newly created tag
    pub tag_id: Uuid,
    /// The name of the tag
    pub tag_name: String,
    /// The commit ID this tag points to
    pub commit_id: Uuid,
}

/// Tag information returned in list and get operations
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct TagInfo {
    /// The tag's unique identifier
    pub tag_id: Uuid,
    /// The name of the tag
    pub tag_name: String,
    /// The commit ID this tag currently points to
    pub commit_id: Uuid,
    /// Optional description of what this tag represents
    pub description: Option<String>,
    /// When the tag was created
    pub created_at: DateTime<Utc>,
    /// When the tag was last updated (moved to different commit or description changed)
    pub updated_at: DateTime<Utc>,
}

/// Response body for GET /api/v1/commit_tags
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct ListTagsResponse {
    /// List of all tags in the user's organization
    pub tags: Vec<TagInfo>,
}

/// Request body for PATCH /api/v1/commit_tags/{tag_name}
///
/// For `description`:
/// - Field absent from JSON → don't change the description
/// - Field present as `null` → clear the description
/// - Field present as `"text"` → set the description to "text"
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct UpdateTagRequest {
    /// Optional new commit ID to move the tag to
    pub commit_id: Option<Uuid>,
    /// Optional new description for the tag. Send `null` to clear an existing description.
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub description: Option<Option<String>>,
}

/// Deserializes a field that distinguishes between absent, null, and present.
/// - Absent → `None` (serde default)
/// - `null` → `Some(None)`
/// - `"value"` → `Some(Some("value"))`
fn deserialize_optional_field<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}
