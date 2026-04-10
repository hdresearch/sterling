use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Request body for POST /api/v1/repositories
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct CreateRepositoryRequest {
    /// The name of the repository (alphanumeric, hyphens, underscores, dots, 1-64 chars)
    pub name: String,
    /// Optional description of the repository
    pub description: Option<String>,
}

/// Response body for POST /api/v1/repositories
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct CreateRepositoryResponse {
    /// The ID of the newly created repository
    pub repo_id: Uuid,
    /// The name of the repository
    pub name: String,
}

/// Repository information returned in list and get operations
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct RepositoryInfo {
    /// The repository's unique identifier
    pub repo_id: Uuid,
    /// The repository name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Whether this repository is publicly visible
    pub is_public: bool,
    /// When the repository was created
    pub created_at: DateTime<Utc>,
}

/// Public repository information (includes owner org name for namespacing)
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct PublicRepositoryInfo {
    /// The repository's unique identifier
    pub repo_id: Uuid,
    /// The owning organization's name (namespace)
    pub org_name: String,
    /// The repository name
    pub name: String,
    /// Full reference: org_name/repo_name
    pub full_name: String,
    /// Optional description
    pub description: Option<String>,
    /// When the repository was created
    pub created_at: DateTime<Utc>,
}

/// Response body for GET /api/v1/public/repositories
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct ListPublicRepositoriesResponse {
    pub repositories: Vec<PublicRepositoryInfo>,
}

/// Request body for PATCH /api/v1/repositories/{repo_name}/visibility
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct SetRepositoryVisibilityRequest {
    /// Whether the repository should be publicly visible
    pub is_public: bool,
}

/// Request body for POST /api/v1/repositories/fork
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct ForkRepositoryRequest {
    /// The organization that owns the source public repository
    pub source_org: String,
    /// The source repository name
    pub source_repo: String,
    /// The tag to fork (e.g. "latest", "v1.0")
    pub source_tag: String,
    /// Name for the new repository in your org (defaults to source_repo if omitted)
    pub repo_name: Option<String>,
    /// Tag name in the new repo (defaults to source_tag if omitted)
    pub tag_name: Option<String>,
}

/// Response body for POST /api/v1/repositories/fork
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct ForkRepositoryResponse {
    /// The new VM that was created from the fork
    pub vm_id: String,
    /// The new commit in your org (snapshot of the forked VM)
    pub commit_id: Uuid,
    /// The new repository name in your org
    pub repo_name: String,
    /// The tag name pointing to the forked commit
    pub tag_name: String,
    /// Full reference: repo_name:tag_name
    pub reference: String,
}

/// Response body for GET /api/v1/repositories
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct ListRepositoriesResponse {
    /// List of all repositories in the user's organization
    pub repositories: Vec<RepositoryInfo>,
}

/// Request body for creating a tag within a repository: POST /api/v1/repositories/{repo_name}/tags
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct CreateRepoTagRequest {
    /// The tag name (e.g. "latest", "v1.0")
    pub tag_name: String,
    /// The commit ID this tag should point to
    pub commit_id: Uuid,
    /// Optional description of what this tag represents
    pub description: Option<String>,
}

/// Response body for POST /api/v1/repositories/{repo_name}/tags
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct CreateRepoTagResponse {
    /// The ID of the newly created tag
    pub tag_id: Uuid,
    /// Full reference in image_name:tag format
    pub reference: String,
    /// The commit ID this tag points to
    pub commit_id: Uuid,
}

/// Tag information within a repository context
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct RepoTagInfo {
    /// The tag's unique identifier
    pub tag_id: Uuid,
    /// The tag name
    pub tag_name: String,
    /// Full reference in image_name:tag format
    pub reference: String,
    /// The commit ID this tag currently points to
    pub commit_id: Uuid,
    /// Optional description
    pub description: Option<String>,
    /// When the tag was created
    pub created_at: DateTime<Utc>,
    /// When the tag was last updated
    pub updated_at: DateTime<Utc>,
}

/// Response body for GET /api/v1/repositories/{repo_name}/tags
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct ListRepoTagsResponse {
    /// The repository name
    pub repository: String,
    /// List of tags in this repository
    pub tags: Vec<RepoTagInfo>,
}

/// Request body for PATCH /api/v1/repositories/{repo_name}/tags/{tag_name}
#[derive(Serialize, Deserialize, Debug, ToSchema)]
pub struct UpdateRepoTagRequest {
    /// Optional new commit ID to move the tag to
    pub commit_id: Option<Uuid>,
    /// Optional new description for the tag. Send `null` to clear.
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub description: Option<Option<String>>,
}

/// Deserializes a field that distinguishes between absent, null, and present.
fn deserialize_optional_field<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}
