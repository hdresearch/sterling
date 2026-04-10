-- migrate:up

-- LLM Proxy billing tables.
-- These live in the main DB alongside users/orgs for real FK relationships.
-- High-volume logs (spend_logs, request_logs) stay in the separate llm_proxy DB.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- Teams for grouping API keys (maps 1:1 to an org)
CREATE TABLE llm_teams (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id      UUID NOT NULL REFERENCES organizations(org_id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    spend       DOUBLE PRECISION NOT NULL DEFAULT 0,
    max_budget  DOUBLE PRECISION,
    credits     DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_llm_teams_org_id ON llm_teams(org_id);

-- Virtual API keys
CREATE TABLE llm_api_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key_hash        TEXT NOT NULL UNIQUE,
    key_prefix      TEXT NOT NULL,
    name            TEXT,
    user_id         UUID NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    team_id         UUID NOT NULL REFERENCES llm_teams(id) ON DELETE CASCADE,
    spend           DOUBLE PRECISION NOT NULL DEFAULT 0,
    credits         DOUBLE PRECISION NOT NULL DEFAULT 0,
    max_budget      DOUBLE PRECISION,
    budget_duration TEXT,
    budget_reset_at TIMESTAMPTZ,
    models          TEXT[] NOT NULL DEFAULT '{}',
    rate_limit_rpm  INT,
    rate_limit_tpm  INT,
    revoked         BOOLEAN NOT NULL DEFAULT false,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_llm_api_keys_key_hash ON llm_api_keys(key_hash);
CREATE INDEX idx_llm_api_keys_user_id ON llm_api_keys(user_id);
CREATE INDEX idx_llm_api_keys_team_id ON llm_api_keys(team_id);

-- Credit transaction ledger
CREATE TABLE llm_credit_transactions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id      UUID REFERENCES llm_api_keys(id) ON DELETE SET NULL,
    team_id         UUID REFERENCES llm_teams(id) ON DELETE SET NULL,
    amount          DOUBLE PRECISION NOT NULL,
    balance_after   DOUBLE PRECISION NOT NULL,
    description     TEXT NOT NULL,
    reference_id    TEXT,
    created_by      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_llm_credit_tx_api_key_id ON llm_credit_transactions(api_key_id);
CREATE INDEX idx_llm_credit_tx_team_id ON llm_credit_transactions(team_id);
CREATE INDEX idx_llm_credit_tx_created_at ON llm_credit_transactions(created_at);

-- migrate:down

DROP TABLE IF EXISTS llm_credit_transactions;
DROP TABLE IF EXISTS llm_api_keys;
DROP TABLE IF EXISTS llm_teams;
