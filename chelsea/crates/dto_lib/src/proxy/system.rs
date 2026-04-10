/// Response struct for GET /version
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ProxyVersion {
    /// Executable identifier; should be "proxy"
    pub executable_name: String,
    /// Current workspace version
    pub workspace_version: String,
    /// Current git hash used for the build
    pub git_hash: String,
}
