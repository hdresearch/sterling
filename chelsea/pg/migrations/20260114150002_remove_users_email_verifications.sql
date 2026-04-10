-- migrate:up

-- Drop the view that depends on email_verified
DROP VIEW IF EXISTS safe_users;

ALTER TABLE users
    DROP COLUMN IF EXISTS email_verified,
    DROP COLUMN IF EXISTS email_verification_nonce,
    DROP COLUMN IF EXISTS email_verification_nonce_expires_at;

-- Recreate the view without email_verified
CREATE VIEW safe_users AS
SELECT
    user_id,
    oauth_provider_user_id,
    email,
    user_name,
    avatar_uri,
    is_human,
    is_active,
    is_deleted,
    is_hdr_employee,
    created_at
FROM users;

-- migrate:down

DROP VIEW IF EXISTS safe_users;

ALTER TABLE users
    ADD COLUMN email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN email_verification_nonce UUID,
    ADD COLUMN email_verification_nonce_expires_at TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '10 minutes');

-- Restore the view with email_verified
CREATE VIEW safe_users AS
SELECT
    user_id,
    oauth_provider_user_id,
    email,
    email_verified,
    user_name,
    avatar_uri,
    is_human,
    is_active,
    is_deleted,
    is_hdr_employee,
    created_at
FROM users;