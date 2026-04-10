-- migrate:up

-- =============================================================================
-- Store user's GitHub org IDs for SSO sync across login providers
-- =============================================================================
-- GitHub org IDs are public integers (not sensitive).
-- Used to sync Vers org memberships even when user signs in with Google
-- (since we don't have a GitHub token to fetch orgs in that case).

ALTER TABLE users ADD COLUMN IF NOT EXISTS github_org_ids INTEGER[];

-- migrate:down
ALTER TABLE users DROP COLUMN IF EXISTS github_org_ids;
