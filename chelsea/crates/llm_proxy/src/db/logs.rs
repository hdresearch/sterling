//! LogDb — spend_logs, request_logs.
//! Separate high-volume, append-only database for request logging and analytics.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use uuid::Uuid;

use super::{ModelSpend, RequestLogRow, SpendSummary, decimal_from_row, to_f64};
use crate::error::DbError;
use billing::db::{PgPool, make_pool};

#[derive(Debug, Clone)]
pub struct LogDb {
    pool: PgPool,
}

impl LogDb {
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        Ok(Self {
            pool: make_pool(url, "log").await?,
        })
    }

    /// Ping the log database. Returns true if reachable.
    pub async fn ping(&self) -> bool {
        match self.pool.get().await {
            Ok(conn) => conn.execute("SELECT 1", &[]).await.is_ok(),
            Err(_) => false,
        }
    }

    pub async fn migrate(&self) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        let migration_sql = include_str!("../../migrations/001_initial.sql");
        conn.batch_execute(migration_sql)
            .await
            .map_err(|e| DbError::Migration(e.to_string()))?;
        tracing::info!("log database migrations applied");
        Ok(())
    }

    /// Ensure request_logs partitions exist for the next `months_ahead` months.
    /// Safe to call repeatedly — uses IF NOT EXISTS.
    pub async fn ensure_partitions(&self, months_ahead: u32) -> Result<(), DbError> {
        let conn = self.pool.get().await?;
        let today = Utc::now().date_naive();

        for i in 0..=months_ahead {
            let start = add_months(today, i);
            let end = add_months(today, i + 1);
            let partition_name = format!("request_logs_{}", start.format("%Y_%m"));

            let sql = format!(
                "CREATE TABLE IF NOT EXISTS {partition_name} \
                 PARTITION OF request_logs \
                 FOR VALUES FROM ('{start}') TO ('{end}')"
            );

            conn.batch_execute(&sql).await.map_err(|e| {
                DbError::Migration(format!("creating partition {partition_name}: {e}"))
            })?;
        }

        tracing::info!("request_logs partitions ensured for {months_ahead} months ahead");
        Ok(())
    }

    /// Spawn a background task that creates partitions every 24 hours.
    pub fn spawn_partition_manager(self: &LogDb, months_ahead: u32) {
        let db = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = db.ensure_partitions(months_ahead).await {
                    tracing::error!("partition management failed: {e}");
                }
                tokio::time::sleep(std::time::Duration::from_secs(86400)).await;
            }
        });
    }

    // ─── Write ───────────────────────────────────────────────────────────

    pub async fn record_request(
        &self,
        request_id: Uuid,
        api_key_id: Uuid,
        team_id: Option<Uuid>,
        model: &str,
        provider: &str,
        prompt_tokens: i32,
        completion_tokens: i32,
        spend: Decimal,
        duration_ms: i32,
        status: &str,
        stop_reason: Option<&str>,
        error_message: Option<&str>,
        request_body: &Value,
        response_body: &Value,
    ) -> Result<(), DbError> {
        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;
        let spend_f64 = to_f64(&spend);

        tx.execute(
            "INSERT INTO spend_logs (id, api_key_id, team_id, model, provider,
                                     prompt_tokens, completion_tokens, total_tokens,
                                     spend, duration_ms, status)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $6::int + $7::int, $8, $9, $10)",
            &[
                &request_id,
                &api_key_id,
                &team_id,
                &model,
                &provider,
                &prompt_tokens,
                &completion_tokens,
                &spend_f64,
                &duration_ms,
                &status,
            ],
        )
        .await?;

        tx.execute(
            "INSERT INTO request_logs (id, api_key_id, team_id, model,
                                       request_body, response_body,
                                       stop_reason, error_message,
                                       prompt_tokens, completion_tokens)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            &[
                &request_id,
                &api_key_id,
                &team_id,
                &model,
                &request_body,
                &response_body,
                &stop_reason,
                &error_message,
                &prompt_tokens,
                &completion_tokens,
            ],
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    // ─── Read ────────────────────────────────────────────────────────────

    pub async fn get_request_log(&self, id: Uuid) -> Result<Option<RequestLogRow>, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_opt(
                "SELECT id, api_key_id, team_id, model, request_body, response_body,
                        stop_reason, error_message, prompt_tokens, completion_tokens
                 FROM request_logs WHERE id = $1",
                &[&id],
            )
            .await?;

        Ok(row.map(|r| RequestLogRow {
            id: r.get("id"),
            api_key_id: r.get("api_key_id"),
            team_id: r.get("team_id"),
            model: r.get("model"),
            request_body: r.get("request_body"),
            response_body: r.get("response_body"),
            stop_reason: r.get("stop_reason"),
            error_message: r.get("error_message"),
            prompt_tokens: r.get("prompt_tokens"),
            completion_tokens: r.get("completion_tokens"),
        }))
    }

    pub async fn get_spend_by_key(
        &self,
        api_key_id: Uuid,
        since: Option<DateTime<Utc>>,
    ) -> Result<SpendSummary, DbError> {
        let conn = self.pool.get().await?;
        let row = conn
            .query_one(
                "SELECT COALESCE(SUM(spend), 0) as total_spend,
                        COALESCE(SUM(prompt_tokens), 0)::bigint as total_prompt_tokens,
                        COALESCE(SUM(completion_tokens), 0)::bigint as total_completion_tokens,
                        COUNT(*) as request_count
                 FROM spend_logs
                 WHERE api_key_id = $1
                   AND ($2::timestamptz IS NULL OR created_at >= $2)",
                &[&api_key_id, &since],
            )
            .await?;

        Ok(SpendSummary {
            total_spend: decimal_from_row(&row, "total_spend"),
            total_prompt_tokens: row.get("total_prompt_tokens"),
            total_completion_tokens: row.get("total_completion_tokens"),
            request_count: row.get("request_count"),
        })
    }

    pub async fn get_spend_by_model(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<ModelSpend>, DbError> {
        let conn = self.pool.get().await?;
        let rows = conn
            .query(
                "SELECT model,
                        COALESCE(SUM(spend), 0) as total_spend,
                        COALESCE(SUM(total_tokens), 0)::bigint as total_tokens,
                        COUNT(*) as request_count
                 FROM spend_logs
                 WHERE ($1::timestamptz IS NULL OR created_at >= $1)
                 GROUP BY model
                 ORDER BY total_spend DESC",
                &[&since],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| ModelSpend {
                model: r.get("model"),
                total_spend: decimal_from_row(r, "total_spend"),
                total_tokens: r.get("total_tokens"),
                request_count: r.get("request_count"),
            })
            .collect())
    }
}

/// Add N months to a date, returning the first day of the resulting month.
fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    let total_months = date.year() as u32 * 12 + (date.month() - 1) + months;
    let year = (total_months / 12) as i32;
    let month = (total_months % 12) + 1;
    NaiveDate::from_ymd_opt(year, month, 1).expect("valid month boundary")
}
