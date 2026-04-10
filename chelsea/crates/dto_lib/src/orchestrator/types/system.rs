/// Response struct for GET api/v1/system/version
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct OrchestratorVersion {
    /// Executable identifier; should be "orchestrator"
    pub executable_name: String,
    /// Current workspace version
    pub workspace_version: String,
    /// Current git hash used for the build
    pub git_hash: String,
}
