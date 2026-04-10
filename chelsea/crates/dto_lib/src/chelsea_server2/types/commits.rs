use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CommitInfo {
    pub commit_id: String,
    pub parent_vm_id: Option<String>,
    pub grandparent_commit_id: Option<String>,
    pub owner_id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub is_public: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ListCommitsResponse {
    pub commits: Vec<CommitInfo>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::IntoParams)]
pub struct ListCommitsQuery {
    /// Maximum number of commits to return (default: 50, max: 100)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of commits to skip (default: 0)
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Request body for PATCH /commits/{commit_id}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateCommitRequest {
    pub is_public: bool,
    /// Optional human-readable name for the commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional description for the commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Query parameters for listing public commits
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::IntoParams)]
pub struct ListPublicCommitsQuery {
    /// Maximum number of commits to return (default: 50, max: 100)
    #[serde(default)]
    pub limit: Option<i64>,
    /// Number of commits to skip (default: 0)
    #[serde(default)]
    pub offset: Option<i64>,
}
