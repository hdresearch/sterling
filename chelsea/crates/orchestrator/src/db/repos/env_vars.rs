use std::collections::HashMap;
use std::future::Future;

use tokio_postgres::types::Type;
use uuid::Uuid;

use crate::db::{DB, DBError};

/// Repository for user-scoped environment variables that are injected into VMs
/// at boot time. Variables are written to `/etc/environment` inside the guest,
/// where they are read by PAM (for SSH sessions) and by the chelsea-agent (for
/// exec'd processes).
///
/// # Lifecycle
///
/// Env vars are a **boot-time-only** concern. They are loaded from this table
/// when a VM is created (new_root / branch / from_commit), serialised into the
/// `VmCreateRequest.env_vars` field, and written to `/etc/environment` inside
/// the guest by the Chelsea vsock agent. Changes made here do **not** propagate
/// to already-running VMs — they take effect on the next VM the user creates.
pub trait EnvVarsRepository {
    /// Retrieves all environment variables for the given user as key-value pairs.
    /// Returns an empty map when the user has no variables configured.
    fn get_by_user_id(
        &self,
        user_id: Uuid,
    ) -> impl Future<Output = Result<HashMap<String, String>, DBError>>;

    /// Upserts one or more environment variables for the given user.
    /// Existing keys are overwritten; keys not mentioned are left untouched.
    fn set(
        &self,
        user_id: Uuid,
        vars: &HashMap<String, String>,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Deletes a single environment variable by key for the given user.
    /// Returns true if a row was deleted, false if the key did not exist.
    fn delete(&self, user_id: Uuid, key: &str) -> impl Future<Output = Result<bool, DBError>>;

    /// Deletes all environment variables for the given user.
    fn delete_all(&self, user_id: Uuid) -> impl Future<Output = Result<(), DBError>>;
}

pub struct EnvVars(DB);

impl DB {
    pub fn env_vars(&self) -> EnvVars {
        EnvVars(self.clone())
    }
}

impl EnvVarsRepository for EnvVars {
    async fn get_by_user_id(&self, user_id: Uuid) -> Result<HashMap<String, String>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT key, value FROM user_env_vars WHERE user_id = $1",
            &[Type::UUID],
            &[&user_id]
        )?;

        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            let key: String = row.try_get("key")?;
            let value: String = row.try_get("value")?;
            map.insert(key, value);
        }

        Ok(map)
    }

    async fn set(&self, user_id: Uuid, vars: &HashMap<String, String>) -> Result<(), DBError> {
        if vars.is_empty() {
            return Ok(());
        }

        let obj = self.0.raw_obj().await;
        for (key, value) in vars {
            obj.execute(
                "INSERT INTO user_env_vars (user_id, key, value)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (user_id, key) DO UPDATE SET value = EXCLUDED.value",
                &[&user_id, &key, &value],
            )
            .await
            .map_err(|e| DBError::from(e))?;
        }

        Ok(())
    }

    async fn delete(&self, user_id: Uuid, key: &str) -> Result<bool, DBError> {
        let obj = self.0.raw_obj().await;
        let count = obj
            .execute(
                "DELETE FROM user_env_vars WHERE user_id = $1 AND key = $2",
                &[&user_id, &key.to_string()],
            )
            .await
            .map_err(|e| DBError::from(e))?;
        Ok(count > 0)
    }

    async fn delete_all(&self, user_id: Uuid) -> Result<(), DBError> {
        let obj = self.0.raw_obj().await;
        obj.execute("DELETE FROM user_env_vars WHERE user_id = $1", &[&user_id])
            .await
            .map_err(|e| DBError::from(e))?;
        Ok(())
    }
}
