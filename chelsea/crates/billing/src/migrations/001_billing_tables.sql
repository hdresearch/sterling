-- Billing tables (for test migrations — production uses dbmate).

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS llm_teams (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id      UUID,
    name        TEXT NOT NULL,
    spend       DOUBLE PRECISION NOT NULL DEFAULT 0,
    max_budget  DOUBLE PRECISION,
    credits     DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_teams_org_id ON llm_teams(org_id) WHERE org_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS llm_api_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key_hash        TEXT NOT NULL UNIQUE,
    key_prefix      TEXT NOT NULL,
    name            TEXT,
    user_id         UUID,
    team_id         UUID REFERENCES llm_teams(id),
    spend           DOUBLE PRECISION NOT NULL DEFAULT 0,
    credits         DOUBLE PRECISION NOT NULL DEFAULT 0,
    max_budget      DOUBLE PRECISION,
    budget_duration TEXT,
    budget_reset_at TIMESTAMPTZ,
    models          TEXT[] NOT NULL DEFAULT '{}',
    revoked         BOOLEAN NOT NULL DEFAULT false,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_llm_api_keys_key_hash ON llm_api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_llm_api_keys_user_id ON llm_api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_llm_api_keys_team_id ON llm_api_keys(team_id);

CREATE TABLE IF NOT EXISTS llm_credit_transactions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    api_key_id      UUID REFERENCES llm_api_keys(id),
    team_id         UUID REFERENCES llm_teams(id),
    amount          DOUBLE PRECISION NOT NULL,
    balance_after   DOUBLE PRECISION NOT NULL,
    description     TEXT NOT NULL,
    reference_id    TEXT,
    created_by      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_llm_credit_tx_api_key_id ON llm_credit_transactions(api_key_id);
CREATE INDEX IF NOT EXISTS idx_llm_credit_tx_team_id ON llm_credit_transactions(team_id);
CREATE INDEX IF NOT EXISTS idx_llm_credit_tx_created_at ON llm_credit_transactions(created_at);
