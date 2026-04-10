const GIT_HASH: &'static str = env!("GIT_HASH");
const WORKSPACE_VERSION: &'static str = include_str!("../../../VERSION");

pub fn git_hash() -> &'static str {
    GIT_HASH.trim()
}

pub fn workspace_version() -> &'static str {
    WORKSPACE_VERSION.trim()
}
