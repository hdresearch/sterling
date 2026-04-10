-- migrate:up

CREATE TABLE IF NOT EXISTS user_env_vars (
    user_id UUID NOT NULL,
    -- Key names are validated to be legal shell variable identifiers to prevent
    -- injection when written to /etc/environment and sourced via profile.d.
    key TEXT NOT NULL CHECK (key ~ '^[A-Za-z_][A-Za-z0-9_]*$' AND length(key) <= 256),
    value TEXT NOT NULL CHECK (length(value) <= 8192),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, key),
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

-- The composite PK (user_id, key) already provides an efficient index for
-- queries filtering only by user_id, so no separate user_id index is needed.

CREATE OR REPLACE FUNCTION update_user_env_vars_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER user_env_vars_updated_at
    BEFORE UPDATE ON user_env_vars
    FOR EACH ROW
    EXECUTE FUNCTION update_user_env_vars_updated_at();

-- migrate:down

DROP TRIGGER IF EXISTS user_env_vars_updated_at ON user_env_vars;
DROP FUNCTION IF EXISTS update_user_env_vars_updated_at();
DROP TABLE IF EXISTS user_env_vars;
