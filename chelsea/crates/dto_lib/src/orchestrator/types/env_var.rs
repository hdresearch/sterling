use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request body for PUT /env_vars — sets (upserts) one or more environment variables.
///
/// # Lifecycle model
///
/// Environment variables are written to `/etc/environment` inside a VM **once
/// at boot time** via a vsock `WriteFile` request. They are **not** live-synced
/// to running VMs. This is intentional: VMs are ephemeral (create → use →
/// destroy/branch), so env var changes naturally take effect on the next VM.
///
/// The `replace` flag exists so callers can atomically express "I want exactly
/// these variables and nothing else" without having to DELETE each stale key
/// individually. Without it, the only way to remove a variable from future VMs
/// is a separate `DELETE /env_vars/{key}` call per key.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct SetEnvVarsRequest {
    /// Key-value pairs to set. Keys must be valid shell identifiers
    /// (`^[A-Za-z_][A-Za-z0-9_]*$`, max 256 chars). Values max 8192 chars.
    ///
    /// When `replace` is false (default): existing keys are overwritten, keys
    /// not mentioned are left untouched (upsert semantics).
    ///
    /// When `replace` is true: all existing variables are deleted first, then
    /// only the provided vars are stored (replace-all semantics).
    pub vars: HashMap<String, String>,

    /// If true, delete all existing variables before writing the new set.
    /// This gives "set exactly these vars" semantics. Default: false (upsert).
    #[serde(default)]
    pub replace: bool,
}

/// Response body for GET /env_vars and PUT /env_vars.
#[derive(ToSchema, Serialize, Deserialize, Debug, Clone)]
pub struct EnvVarsResponse {
    /// All environment variables currently set for the authenticated user.
    pub vars: HashMap<String, String>,
}
