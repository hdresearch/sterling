//! Team operations on `llm_teams`.

use rust_decimal::Decimal;
use uuid::Uuid;

use super::{BillingDb, TeamRow, decimal_from_row, decimal_opt_from_row, to_f64};
use crate::error::DbError;

impl BillingDb {
    pub async fn create_team(
        &self,
        id: Uuid,
        name: &str,
        org_id: Option<Uuid>,
    ) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        conn.execute(
            "INSERT INTO llm_teams (id, name, org_id) VALUES ($1, $2, $3)",
            &[&id, &name, &org_id],
        )
        .await?;
        Ok(())
    }

    pub async fn get_team(&self, id: Uuid) -> Result<Option<TeamRow>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT id, org_id, name, spend, credits, max_budget FROM llm_teams WHERE id = $1",
                &[&id],
            )
            .await?;

        Ok(row.map(|r| TeamRow {
            id: r.get("id"),
            org_id: r.get("org_id"),
            name: r.get("name"),
            spend: decimal_from_row(&r, "spend"),
            credits: decimal_from_row(&r, "credits"),
            max_budget: decimal_opt_from_row(&r, "max_budget"),
        }))
    }

    pub async fn increment_team_spend(
        &self,
        team_id: Uuid,
        amount: Decimal,
    ) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        let amount_f64 = to_f64(&amount);
        conn.execute(
            "UPDATE llm_teams SET spend = spend + $1 WHERE id = $2",
            &[&amount_f64, &team_id],
        )
        .await?;
        Ok(())
    }

    /// Find or create an `llm_teams` row for an org.
    pub async fn find_or_create_team(&self, org_id: Uuid) -> Result<Uuid, DbError> {
        let conn = self.pool.get().await?;

        let existing = conn
            .query_opt("SELECT id FROM llm_teams WHERE org_id = $1", &[&org_id])
            .await?;

        if let Some(row) = existing {
            return Ok(row.get("id"));
        }

        let team_id = Uuid::new_v4();
        conn.execute(
            "INSERT INTO llm_teams (id, org_id, name) VALUES ($1, $2, $3)
             ON CONFLICT (org_id) WHERE org_id IS NOT NULL DO NOTHING",
            &[
                &team_id,
                &org_id,
                &format!("org-{}", &org_id.to_string()[..8]),
            ],
        )
        .await?;

        let row = conn
            .query_one("SELECT id FROM llm_teams WHERE org_id = $1", &[&org_id])
            .await?;
        Ok(row.get("id"))
    }
}
