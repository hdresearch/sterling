use std::collections::HashMap;

use dto_lib::orchestrator::env_var::EnvVarsResponse;
use thiserror::Error;

use crate::{
    action::Action,
    db::{ApiKeyEntity, DBError, EnvVarsRepository},
};

/// Maximum number of variables that can be set in a single request.
const MAX_VARS_PER_REQUEST: usize = 100;

/// Maximum total number of variables per user.
const MAX_VARS_PER_USER: usize = 256;

/// Sets (upserts) user environment variables, optionally replacing all existing ones.
///
/// # Lifecycle model
///
/// Env vars are a **boot-time-only** concern. They are loaded from the database
/// when a VM is created and written to `/etc/environment` inside the guest via
/// the vsock agent's `WriteFile` command. Changes made here do **not** propagate
/// to already-running VMs — they take effect on the next VM the user creates.
///
/// The `replace` flag exists because without it, removing a variable requires a
/// separate `DELETE /env_vars/{key}` call per key. With `replace: true`, the
/// caller can express "I want exactly these variables and nothing else" in a
/// single request — any keys not present in `vars` are deleted.
#[derive(Debug, Clone)]
pub struct SetEnvVars {
    pub api_key: ApiKeyEntity,
    pub vars: HashMap<String, String>,
    pub replace: bool,
}

impl SetEnvVars {
    pub fn new(api_key: ApiKeyEntity, vars: HashMap<String, String>, replace: bool) -> Self {
        Self {
            api_key,
            vars,
            replace,
        }
    }
}

#[derive(Debug, Error)]
pub enum SetEnvVarsError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("{0}")]
    Validation(String),
}

impl Action for SetEnvVars {
    type Response = EnvVarsResponse;
    type Error = SetEnvVarsError;
    const ACTION_ID: &'static str = "env_vars.set";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // Validate request size
        if self.vars.is_empty() && !self.replace {
            return Err(SetEnvVarsError::Validation(
                "vars must not be empty (use replace: true with empty vars to clear all)"
                    .to_string(),
            ));
        }
        if self.vars.len() > MAX_VARS_PER_REQUEST {
            return Err(SetEnvVarsError::Validation(format!(
                "too many variables in request (max {MAX_VARS_PER_REQUEST})"
            )));
        }

        // Validate key format (same rules as DB CHECK constraint)
        for (key, value) in &self.vars {
            if key.len() > 256 {
                return Err(SetEnvVarsError::Validation(format!(
                    "key '{}...' exceeds 256 character limit",
                    &key[..32]
                )));
            }
            if !is_valid_shell_identifier(key) {
                return Err(SetEnvVarsError::Validation(format!(
                    "key '{key}' is not a valid shell identifier"
                )));
            }
            if value.len() > 8192 {
                return Err(SetEnvVarsError::Validation(format!(
                    "value for key '{key}' exceeds 8192 character limit"
                )));
            }
        }

        let user_id = self.api_key.user_id();

        if self.replace {
            // Replace-all: delete everything first, then insert the new set.
            // This lets callers express "I want exactly these vars" atomically.
            ctx.db.env_vars().delete_all(user_id).await?;
        } else {
            // Upsert: check total count won't exceed limit
            let existing = ctx.db.env_vars().get_by_user_id(user_id).await?;
            let new_keys: usize = self
                .vars
                .keys()
                .filter(|k| !existing.contains_key(*k))
                .count();
            if existing.len() + new_keys > MAX_VARS_PER_USER {
                return Err(SetEnvVarsError::Validation(format!(
                    "would exceed maximum of {MAX_VARS_PER_USER} environment variables per user"
                )));
            }
        }

        // Insert/upsert the provided vars
        if !self.vars.is_empty() {
            ctx.db.env_vars().set(user_id, &self.vars).await?;
        }

        // Return full set
        let vars = ctx.db.env_vars().get_by_user_id(user_id).await?;
        Ok(EnvVarsResponse { vars })
    }
}

/// Check if a string is a valid shell variable identifier: `^[A-Za-z_][A-Za-z0-9_]*$`
fn is_valid_shell_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::is_valid_shell_identifier;

    #[test]
    fn valid_identifiers() {
        assert!(is_valid_shell_identifier("FOO"));
        assert!(is_valid_shell_identifier("_BAR"));
        assert!(is_valid_shell_identifier("a1_b2"));
        assert!(is_valid_shell_identifier("DATABASE_URL"));
    }

    #[test]
    fn invalid_identifiers() {
        assert!(!is_valid_shell_identifier(""));
        assert!(!is_valid_shell_identifier("1BAD"));
        assert!(!is_valid_shell_identifier("BAD KEY"));
        assert!(!is_valid_shell_identifier("BAD;KEY"));
        assert!(!is_valid_shell_identifier("BAD-KEY"));
        assert!(!is_valid_shell_identifier("BAD.KEY"));
    }

    #[test]
    fn single_char_identifiers() {
        assert!(is_valid_shell_identifier("A"));
        assert!(is_valid_shell_identifier("_"));
        assert!(!is_valid_shell_identifier("1"));
        assert!(!is_valid_shell_identifier("-"));
    }

    #[test]
    fn underscore_only_identifier() {
        assert!(is_valid_shell_identifier("_"));
        assert!(is_valid_shell_identifier("__"));
        assert!(is_valid_shell_identifier("___FOO"));
    }

    #[test]
    fn unicode_rejected() {
        assert!(!is_valid_shell_identifier("café"));
        assert!(!is_valid_shell_identifier("变量"));
    }
}
