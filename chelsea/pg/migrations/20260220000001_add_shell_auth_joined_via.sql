-- migrate:up
-- Add 'shell_auth' as a valid joined_via value for CLI/agent users
-- Required by vers-landing PR #253 (aliased shell auth users as org members)

ALTER TABLE user_org_memberships
    DROP CONSTRAINT IF EXISTS user_org_memberships_joined_via_check;

ALTER TABLE user_org_memberships
    ADD CONSTRAINT user_org_memberships_joined_via_check
    CHECK (joined_via IS NULL OR joined_via IN ('creator', 'ownership_transfer', 'invite', 'email_domain', 'github_org', 'shell_auth'));

-- migrate:down
ALTER TABLE user_org_memberships
    DROP CONSTRAINT IF EXISTS user_org_memberships_joined_via_check;
ALTER TABLE user_org_memberships
    ADD CONSTRAINT user_org_memberships_joined_via_check
    CHECK (joined_via IS NULL OR joined_via IN ('creator', 'ownership_transfer', 'invite', 'email_domain', 'github_org'));
