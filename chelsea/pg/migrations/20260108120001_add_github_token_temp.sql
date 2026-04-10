-- migrate:up

-- Add temporary column to store GitHub access token during account setup flow
-- This token is used once to check HDR employee status, then cleared
ALTER TABLE oauth_user_profiles ADD COLUMN IF NOT EXISTS github_access_token_temp TEXT;

-- migrate:down
ALTER TABLE oauth_user_profiles DROP COLUMN IF EXISTS github_access_token_temp;
