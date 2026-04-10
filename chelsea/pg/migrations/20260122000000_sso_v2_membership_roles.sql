-- migrate:up

-- =============================================================================
-- SSO v2: Add membership role system and join tracking
-- =============================================================================
-- Required by vers-landing sso-v2-simple branch.
-- Vers-landing migration 20260122000001_sso_v2_simple.sql depends on these columns.

-- Track how user joined the org
ALTER TABLE user_org_memberships
    ADD COLUMN IF NOT EXISTS joined_via TEXT
    CHECK (joined_via IS NULL OR joined_via IN ('creator', 'ownership_transfer', 'invite', 'email_domain', 'github_org'));

-- Role system (admin can manage members, member is default)
ALTER TABLE user_org_memberships
    ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'member'
    CHECK (role IN ('admin', 'member'));

-- Backfill: earliest member of each org becomes admin (proxy for creator)
WITH first_members AS (
    SELECT DISTINCT ON (org_id) id, org_id, user_id
    FROM user_org_memberships
    WHERE is_deleted = FALSE
    ORDER BY org_id, created_at ASC
)
UPDATE user_org_memberships m
SET role = 'admin', joined_via = 'creator'
FROM first_members fm
WHERE m.id = fm.id;

-- =============================================================================
-- Add default org preference to users
-- =============================================================================

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS default_org_id UUID REFERENCES organizations(org_id) ON DELETE SET NULL;

-- migrate:down
ALTER TABLE users DROP COLUMN IF EXISTS default_org_id;
ALTER TABLE user_org_memberships DROP COLUMN IF EXISTS role;
ALTER TABLE user_org_memberships DROP COLUMN IF EXISTS joined_via;
