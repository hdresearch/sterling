//! API key operations on `llm_api_keys`.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::{
    ApiKeyRow, BillingDb, VersApiKeyRow, decimal_from_row, decimal_opt_from_row, to_f64_opt,
};
use crate::error::DbError;

impl BillingDb {
    pub async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyRow>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT k.id, k.key_prefix, k.name, k.user_id, k.team_id,
                        k.spend, k.max_budget, k.models, k.revoked, k.expires_at,
                        COALESCE(t.credits, 0) AS credits,
                        CASE
                            WHEN k.revoked THEN 'key_revoked'
                            WHEN k.expires_at IS NOT NULL AND now() > k.expires_at THEN 'key_expired'
                            WHEN t.id IS NULL THEN 'no_credits'
                            WHEN t.credits = 0 AND t.max_budget IS NULL THEN 'no_credits'
                            WHEN t.credits > 0 AND (t.credits - t.spend) <= 0 THEN 'credits_exhausted'
                            WHEN t.max_budget IS NOT NULL AND t.spend >= t.max_budget THEN 'budget_exceeded'
                            WHEN k.max_budget IS NOT NULL AND k.spend >= k.max_budget THEN 'budget_exceeded'
                            ELSE NULL
                        END AS deny_reason
                 FROM llm_api_keys k
                 LEFT JOIN llm_teams t ON k.team_id = t.id
                 WHERE k.key_hash = $1",
                &[&key_hash],
            )
            .await?;

        Ok(row.as_ref().map(api_key_from_row))
    }

    pub async fn get_api_key_by_id(&self, id: Uuid) -> Result<Option<ApiKeyRow>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT k.id, k.key_prefix, k.name, k.user_id, k.team_id,
                        k.spend, k.max_budget, k.models, k.revoked, k.expires_at,
                        COALESCE(t.credits, 0) AS credits,
                        NULL::text AS deny_reason
                 FROM llm_api_keys k
                 LEFT JOIN llm_teams t ON k.team_id = t.id
                 WHERE k.id = $1",
                &[&id],
            )
            .await?;
        Ok(row.as_ref().map(api_key_from_row))
    }

    pub async fn create_api_key(
        &self,
        id: Uuid,
        key_hash: &str,
        key_prefix: &str,
        name: Option<&str>,
        user_id: Option<Uuid>,
        team_id: Option<Uuid>,
        max_budget: Option<Decimal>,
        models: &[String],
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        conn.execute(
            "INSERT INTO llm_api_keys (id, key_hash, key_prefix, name, user_id, team_id, max_budget, models, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            &[&id, &key_hash, &key_prefix, &name, &user_id, &team_id, &to_f64_opt(&max_budget), &models, &expires_at],
        )
        .await?;
        Ok(())
    }

    pub async fn revoke_api_key(&self, id: Uuid) -> Result<bool, DbError> {
        let conn = self.pool.get().await?;
        let rows = conn
            .execute(
                "UPDATE llm_api_keys SET revoked = true WHERE id = $1 AND revoked = false",
                &[&id],
            )
            .await?;
        Ok(rows > 0)
    }

    pub async fn list_api_keys(&self, team_id: Option<Uuid>) -> Result<Vec<ApiKeyRow>, DbError> {
        let conn = self.pool.get().await?;
        let rows = conn
            .query(
                "SELECT k.id, k.key_prefix, k.name, k.user_id, k.team_id,
                        k.spend, k.max_budget, k.models, k.revoked, k.expires_at,
                        COALESCE(t.credits, 0) AS credits,
                        CASE
                            WHEN k.revoked THEN 'key_revoked'
                            WHEN k.expires_at IS NOT NULL AND now() > k.expires_at THEN 'key_expired'
                            WHEN t.id IS NULL THEN 'no_credits'
                            WHEN t.credits = 0 AND t.max_budget IS NULL THEN 'no_credits'
                            WHEN t.credits > 0 AND (t.credits - t.spend) <= 0 THEN 'credits_exhausted'
                            WHEN t.max_budget IS NOT NULL AND t.spend >= t.max_budget THEN 'budget_exceeded'
                            WHEN k.max_budget IS NOT NULL AND k.spend >= k.max_budget THEN 'budget_exceeded'
                            ELSE NULL
                        END AS deny_reason
                 FROM llm_api_keys k
                 LEFT JOIN llm_teams t ON k.team_id = t.id
                 WHERE ($1::uuid IS NULL OR k.team_id = $1)
                 ORDER BY k.created_at DESC",
                &[&team_id],
            )
            .await?;

        Ok(rows.iter().map(api_key_from_row).collect())
    }

    pub async fn update_api_key_budget(
        &self,
        id: Uuid,
        max_budget: Option<Decimal>,
        reset_spend: bool,
    ) -> Result<bool, DbError> {
        let conn = self.pool.get().await?;
        let budget_f64 = to_f64_opt(&max_budget);
        let rows = if reset_spend {
            conn.execute(
                "UPDATE llm_api_keys SET max_budget = $1, spend = 0 WHERE id = $2",
                &[&budget_f64, &id],
            )
            .await?
        } else {
            conn.execute(
                "UPDATE llm_api_keys SET max_budget = $1 WHERE id = $2",
                &[&budget_f64, &id],
            )
            .await?
        };
        Ok(rows > 0)
    }

    pub async fn increment_key_spend(&self, key_id: Uuid, amount: Decimal) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        let amount_f64 = super::to_f64(&amount);
        conn.execute(
            "UPDATE llm_api_keys SET spend = spend + $1 WHERE id = $2",
            &[&amount_f64, &key_id],
        )
        .await?;
        Ok(())
    }

    /// Look up a Vers platform API key for key exchange.
    pub async fn get_vers_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<VersApiKeyRow>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT api_key_id, user_id, org_id, label, key_iter, key_salt, key_hash
                 FROM api_keys
                 WHERE api_key_id = $1
                   AND is_active = TRUE
                   AND is_deleted = FALSE
                   AND revoked_at IS NULL
                   AND (expires_at IS NULL OR expires_at > NOW())",
                &[&api_key_id],
            )
            .await?;

        Ok(row.map(|r| VersApiKeyRow {
            user_id: r.get("user_id"),
            org_id: r.get("org_id"),
            label: r.get("label"),
            iterations: r.get("key_iter"),
            salt: r.get("key_salt"),
            hash: r.get("key_hash"),
        }))
    }
}

fn api_key_from_row(r: &tokio_postgres::Row) -> ApiKeyRow {
    ApiKeyRow {
        id: r.get("id"),
        key_prefix: r.get("key_prefix"),
        name: r.get("name"),
        user_id: r.get("user_id"),
        team_id: r.get("team_id"),
        spend: decimal_from_row(r, "spend"),
        credits: decimal_from_row(r, "credits"),
        max_budget: decimal_opt_from_row(r, "max_budget"),
        models: r.get("models"),
        revoked: r.get("revoked"),
        expires_at: r.get("expires_at"),
        deny_reason: r.get("deny_reason"),
    }
}
