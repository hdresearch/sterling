use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::db::{DB, DBError};

pub trait ApiKeysRepository {
    fn insert(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        label: &str,
        key_algo: &str,
        key_iter: i32,
        key_salt: &str,
        key_hash: &str,
        created_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<ApiKeyEntity, DBError>>;

    fn get_by_hash(
        &self,
        key_hash: &str,
    ) -> impl Future<Output = Result<Option<ApiKeyEntity>, DBError>>;

    /// Fetch only keys that are currently valid for use:
    /// - `is_active = true`
    /// - `is_deleted = false`
    /// - `revoked_at IS NULL`
    /// - `expires_at IS NULL OR expires_at > now()`
    fn get_valid_by_hash(
        &self,
        key_hash: &str,
    ) -> impl Future<Output = Result<Option<ApiKeyEntity>, DBError>>;

    fn get_by_id(
        &self,
        api_key_id: Uuid,
    ) -> impl Future<Output = Result<Option<ApiKeyEntity>, DBError>>;

    fn revoke(
        &self,
        api_key_id: Uuid,
        when: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), DBError>>;

    fn set_deleted(
        &self,
        api_key_id: Uuid,
        when: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), DBError>>;

    fn set_active(
        &self,
        api_key_id: Uuid,
        active: bool,
    ) -> impl Future<Output = Result<(), DBError>>;

    fn list_valid(&self) -> impl Future<Output = Result<Vec<ApiKeyEntity>, DBError>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntity {
    api_key_id: Uuid,
    user_id: Uuid,
    org_id: Uuid,
    label: String,
    key_algo: String,
    key_iter: i32,
    key_salt: String,
    key_hash: String,
    is_active: bool,
    is_deleted: bool,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
}

impl ApiKeyEntity {
    pub fn id(&self) -> Uuid {
        self.api_key_id
    }

    pub fn salt(&self) -> &str {
        &self.key_salt
    }

    pub fn iter(&self) -> i32 {
        self.key_iter
    }

    pub fn hash(&self) -> &str {
        &self.key_hash
    }

    pub fn algo(&self) -> &str {
        &self.key_algo
    }

    pub fn org_id(&self) -> Uuid {
        self.org_id
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }
}

impl TryFrom<Row> for ApiKeyEntity {
    type Error = ();

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(Self {
            api_key_id: row.get("api_key_id"),
            user_id: row.get("user_id"),
            org_id: row.get("org_id"),
            label: row.get("label"),
            key_algo: row.get("key_algo"),
            key_iter: row.get("key_iter"),
            key_salt: row.get("key_salt"),
            key_hash: row.get("key_hash"),
            is_active: row.get("is_active"),
            is_deleted: row.get("is_deleted"),
            created_at: row.get("created_at"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
            deleted_at: row.get("deleted_at"),
        })
    }
}

pub struct ApiKeys(DB);

impl DB {
    pub fn keys(&self) -> ApiKeys {
        ApiKeys(self.clone())
    }
}

impl ApiKeysRepository for ApiKeys {
    async fn insert(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        label: &str,
        key_algo: &str,
        key_iter: i32,
        key_salt: &str,
        key_hash: &str,
        created_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<ApiKeyEntity, DBError> {
        let api_key_id = Uuid::new_v4();
        let rows_affected = execute_sql!(
            self.0,
            "INSERT INTO api_keys (
                api_key_id, user_id, org_id, label, key_algo, key_iter, key_salt, key_hash,
                is_active, is_deleted, created_at, expires_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
            &[
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::TEXT,
                Type::INT4,
                Type::TEXT,
                Type::TEXT,
                Type::BOOL,
                Type::BOOL,
                Type::TIMESTAMPTZ,
                Type::TIMESTAMPTZ,
            ],
            &[
                &api_key_id,
                &user_id,
                &org_id,
                &label,
                &key_algo,
                &key_iter,
                &key_salt,
                &key_hash,
                &true,
                &false,
                &created_at,
                &expires_at,
            ]
        )?;
        debug_assert!(rows_affected == 1);

        Ok(ApiKeyEntity {
            api_key_id,
            user_id,
            org_id,
            label: label.to_string(),
            key_algo: key_algo.to_string(),
            key_iter,
            key_salt: key_salt.to_string(),
            key_hash: key_hash.to_string(),
            is_active: true,
            is_deleted: false,
            created_at,
            expires_at,
            revoked_at: None,
            deleted_at: None,
        })
    }

    async fn get_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyEntity>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT * FROM api_keys WHERE key_hash = $1",
            &[Type::TEXT],
            &[&key_hash]
        )?;
        Ok(maybe_row.map(|row| TryInto::<ApiKeyEntity>::try_into(row).unwrap()))
    }

    async fn get_by_id(&self, api_key_id: Uuid) -> Result<Option<ApiKeyEntity>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT * FROM api_keys WHERE api_key_id = $1",
            &[Type::UUID],
            &[&api_key_id]
        )?;
        Ok(maybe_row.map(|row| TryInto::<ApiKeyEntity>::try_into(row).unwrap()))
    }

    async fn get_valid_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyEntity>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT * FROM api_keys
             WHERE key_hash = $1
               AND is_active = TRUE
               AND is_deleted = FALSE
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > NOW())",
            &[Type::TEXT],
            &[&key_hash]
        )?;
        Ok(maybe_row.map(|row| TryInto::<ApiKeyEntity>::try_into(row).unwrap()))
    }

    async fn revoke(&self, api_key_id: Uuid, when: DateTime<Utc>) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE api_keys SET is_active = FALSE, revoked_at = $2 WHERE api_key_id = $1",
            &[Type::UUID, Type::TIMESTAMPTZ],
            &[&api_key_id, &when]
        )?;
        debug_assert!(rows == 1);
        Ok(())
    }

    async fn set_deleted(&self, api_key_id: Uuid, when: DateTime<Utc>) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE api_keys SET is_deleted = TRUE, deleted_at = $2 WHERE api_key_id = $1",
            &[Type::UUID, Type::TIMESTAMPTZ],
            &[&api_key_id, &when]
        )?;
        debug_assert!(rows == 1);
        Ok(())
    }

    async fn set_active(&self, api_key_id: Uuid, active: bool) -> Result<(), DBError> {
        let rows = execute_sql!(
            self.0,
            "UPDATE api_keys SET is_active = $2 WHERE api_key_id = $1",
            &[Type::UUID, Type::BOOL],
            &[&api_key_id, &active]
        )?;
        debug_assert!(rows == 1);
        Ok(())
    }

    async fn list_valid(&self) -> Result<Vec<ApiKeyEntity>, DBError> {
        let rows = query_sql!(
            self.0,
            "SELECT * FROM api_keys
             WHERE is_active = TRUE
               AND is_deleted = FALSE
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > NOW())"
        )?;

        Ok(rows.into_iter().map(|r| r.try_into().unwrap()).collect())
    }
}
