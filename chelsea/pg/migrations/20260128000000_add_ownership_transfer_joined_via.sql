-- migrate:up
-- Add 'ownership_transfer' as a valid joined_via value
-- This distinguishes between original org creators and users who received ownership via transfer

-- Drop the old constraint and recreate with the new value
ALTER TABLE user_org_memberships
    DROP CONSTRAINT IF EXISTS user_org_memberships_joined_via_check;

ALTER TABLE user_org_memberships
    ADD CONSTRAINT user_org_memberships_joined_via_check
    CHECK (joined_via IS NULL OR joined_via IN ('creator', 'ownership_transfer', 'invite', 'email_domain', 'github_org'));

-- migrate:down
ALTER TABLE user_org_memberships
    DROP CONSTRAINT IF EXISTS user_org_memberships_joined_via_check;
ALTER TABLE user_org_memberships
    ADD CONSTRAINT user_org_memberships_joined_via_check
    CHECK (joined_via IS NULL OR joined_via IN ('creator', 'invite', 'email_domain', 'github_org'));
