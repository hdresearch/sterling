-- migrate:up

-- Add is_hdr_employee column to track HDR Research employees
-- This is set at sign-in time based on email domain (@hdr.is) or GitHub org membership (hdresearch)
ALTER TABLE users ADD COLUMN is_hdr_employee BOOLEAN NOT NULL DEFAULT FALSE;

-- Update the safe_users view to include the new column
DROP VIEW IF EXISTS safe_users;
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

-- migrate:down
ALTER TABLE users DROP COLUMN is_hdr_employee;
DROP VIEW IF EXISTS safe_users;
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
    created_at
FROM users;
