//! Credit operations on `llm_teams` and `llm_credit_transactions`.

use rust_decimal::Decimal;
use uuid::Uuid;

use super::{BillingDb, CreditTransaction, decimal_from_row, to_f64};
use crate::error::DbError;

impl BillingDb {
    pub async fn add_credits(
        &self,
        api_key_id: Uuid,
        amount: Decimal,
        description: &str,
        reference_id: Option<&str>,
        created_by: &str,
    ) -> Result<Decimal, DbError> {
        if amount <= Decimal::ZERO {
            return Err(DbError::InvalidCreditAmount);
        }

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        let amount_f64 = to_f64(&amount);
        let row = tx
            .query_one(
                "UPDATE llm_api_keys SET credits = credits + $1 WHERE id = $2 RETURNING credits",
                &[&amount_f64, &api_key_id],
            )
            .await?;
        let new_balance = decimal_from_row(&row, "credits");
        let new_balance_f64 = to_f64(&new_balance);

        tx.execute(
            "INSERT INTO llm_credit_transactions (api_key_id, amount, balance_after, description, reference_id, created_by)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[&api_key_id, &amount_f64, &new_balance_f64, &description, &reference_id, &created_by],
        )
        .await?;

        tx.commit().await?;
        Ok(new_balance)
    }

    pub async fn add_team_credits(
        &self,
        team_id: Uuid,
        amount: Decimal,
        description: &str,
        reference_id: Option<&str>,
        created_by: &str,
    ) -> Result<Decimal, DbError> {
        if amount <= Decimal::ZERO {
            return Err(DbError::InvalidCreditAmount);
        }

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        let amount_f64 = to_f64(&amount);
        let row = tx
            .query_one(
                "UPDATE llm_teams SET credits = credits + $1 WHERE id = $2 RETURNING credits",
                &[&amount_f64, &team_id],
            )
            .await?;
        let new_balance = decimal_from_row(&row, "credits");
        let new_balance_f64 = to_f64(&new_balance);

        tx.execute(
            "INSERT INTO llm_credit_transactions (team_id, amount, balance_after, description, reference_id, created_by)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[&team_id, &amount_f64, &new_balance_f64, &description, &reference_id, &created_by],
        )
        .await?;

        tx.commit().await?;
        Ok(new_balance)
    }

    pub async fn get_credit_history(
        &self,
        api_key_id: Uuid,
    ) -> Result<Vec<CreditTransaction>, DbError> {
        let conn = self.pool.get().await?;
        let rows = conn
            .query(
                "SELECT id, amount, balance_after, description, reference_id, created_by, created_at
                 FROM llm_credit_transactions
                 WHERE api_key_id = $1
                 ORDER BY created_at DESC",
                &[&api_key_id],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| CreditTransaction {
                id: r.get("id"),
                amount: decimal_from_row(r, "amount"),
                balance_after: decimal_from_row(r, "balance_after"),
                description: r.get("description"),
                reference_id: r.get("reference_id"),
                created_by: r.get("created_by"),
                created_at: r.get("created_at"),
            })
            .collect())
    }
}
