-- LLM Proxy log tables.
-- Billing tables (api_keys, teams, credit_transactions) live in the main DB.
-- This DB only stores high-volume, append-only log data.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- Lightweight spend logs (per-request metadata, heavily queried for analytics)
CREATE TABLE IF NOT EXISTS spend_logs (
    id                  UUID PRIMARY KEY,
    api_key_id          UUID NOT NULL,      -- references llm_api_keys.id in main DB (cross-DB, no FK)
    team_id             UUID,               -- references llm_teams.id in main DB
    model               TEXT NOT NULL,
    provider            TEXT NOT NULL,
    prompt_tokens       INT NOT NULL DEFAULT 0,
    completion_tokens   INT NOT NULL DEFAULT 0,
    total_tokens        INT NOT NULL DEFAULT 0,
    spend               DOUBLE PRECISION NOT NULL DEFAULT 0,
    duration_ms         INT NOT NULL DEFAULT 0,
    status              TEXT NOT NULL DEFAULT 'success',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_spend_logs_api_key_id ON spend_logs(api_key_id);
CREATE INDEX IF NOT EXISTS idx_spend_logs_team_id ON spend_logs(team_id);
CREATE INDEX IF NOT EXISTS idx_spend_logs_model ON spend_logs(model);
CREATE INDEX IF NOT EXISTS idx_spend_logs_created_at ON spend_logs(created_at);

-- Full request/response payloads (partitioned by month for lifecycle management)
CREATE TABLE IF NOT EXISTS request_logs (
    id                  UUID NOT NULL,
    api_key_id          UUID NOT NULL,
    team_id             UUID,
    model               TEXT NOT NULL,
    request_body        JSONB NOT NULL,
    response_body       JSONB NOT NULL DEFAULT '{}',
    prompt_tokens       INT NOT NULL DEFAULT 0,
    completion_tokens   INT NOT NULL DEFAULT 0,
    stop_reason         TEXT,
    error_message       TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

CREATE INDEX IF NOT EXISTS idx_request_logs_api_key_id ON request_logs(api_key_id);
CREATE INDEX IF NOT EXISTS idx_request_logs_team_id ON request_logs(team_id);
CREATE INDEX IF NOT EXISTS idx_request_logs_model ON request_logs(model);
CREATE INDEX IF NOT EXISTS idx_request_logs_stop_reason ON request_logs(stop_reason);

-- Create partitions for current and next month
DO $$
DECLARE
    current_start DATE := date_trunc('month', CURRENT_DATE);
    next_start DATE := date_trunc('month', CURRENT_DATE + INTERVAL '1 month');
    after_next DATE := date_trunc('month', CURRENT_DATE + INTERVAL '2 months');
BEGIN
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS request_logs_%s PARTITION OF request_logs FOR VALUES FROM (%L) TO (%L)',
        to_char(current_start, 'YYYY_MM'),
        current_start,
        next_start
    );
    EXECUTE format(
        'CREATE TABLE IF NOT EXISTS request_logs_%s PARTITION OF request_logs FOR VALUES FROM (%L) TO (%L)',
        to_char(next_start, 'YYYY_MM'),
        next_start,
        after_next
    );
END $$;
