-- migrate:up

CREATE TABLE IF NOT EXISTS user_ssh_keys (
    key_id              UUID        PRIMARY KEY,
    user_id             UUID        NOT NULL,
    public_key_block    TEXT        NOT NULL UNIQUE,
    public_key_verified BOOLEAN     NOT NULL DEFAULT FALSE,
    is_active           BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    verified_at         TIMESTAMPTZ NULL,
    last_seen_at        TIMESTAMPTZ NULL,
    FOREIGN KEY (user_id) REFERENCES users(user_id)
);

CREATE TABLE user_email_verifications (
    email_id     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL,
    email        CITEXT NOT NULL, -- snapshot semantics... auditing.
    body         TEXT, -- initial signup will be null, because templates.
    verified     BOOLEAN NOT NULL DEFAULT FALSE,
    nonce        UUID NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '10 minutes'),
    verified_at  TIMESTAMPTZ NULL,
    FOREIGN KEY (user_id) REFERENCES users(user_id)
);

CREATE INDEX IF NOT EXISTS idx_user_ssh_keys_1
    ON user_ssh_keys(user_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_email_verifications_1
    ON user_email_verifications(user_id, nonce);

-- migrate:down

DROP INDEX IF EXISTS idx_user_email_verifications_1;
DROP INDEX IF EXISTS idx_user_ssh_keys_1;

DROP TABLE IF EXISTS user_ssh_keys;
DROP TABLE IF EXISTS user_email_verifications;
