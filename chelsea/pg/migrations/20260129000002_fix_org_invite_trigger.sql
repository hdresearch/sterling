-- migrate:up

-- =============================================================================
-- Fix: check_org_invites_integrity trigger
-- =============================================================================
-- Was checking if user has ANY active membership instead of checking membership
-- for the SPECIFIC org being invited to. This caused "Cannot invite a user to an
-- organization they have accepted an invite for" error when re-inviting users who
-- had left an org but had memberships in other orgs.

CREATE OR REPLACE FUNCTION check_org_invites_integrity()
RETURNS TRIGGER
AS $$
BEGIN
    -- Count non-expired invites for the same user_email and org
    IF (
        SELECT COUNT(*)
        FROM organization_invites
        WHERE org_id = NEW.org_id
        AND user_email = NEW.user_email
        AND expires_at > CURRENT_TIMESTAMP
    ) >= 5 THEN
        RAISE EXCEPTION 'Maximum of 5 non-expired invites allowed per user email and organization. Invite limit exceed for %.', NEW.user_email;
    END IF;

    -- Check if the user has already accepted an invite for that org
    -- AND they are currently an active member of THAT SPECIFIC org
    IF (
        SELECT COUNT(*)
        FROM organization_invites
        JOIN users u ON u.email = NEW.user_email
        JOIN user_org_memberships um ON um.user_id = u.user_id
        WHERE organization_invites.org_id = NEW.org_id
        AND um.org_id = NEW.org_id  -- FIX: Check membership for the specific org
        AND um.is_deleted = FALSE
        AND um.is_active = TRUE     -- FIX: Also check is_active
        AND organization_invites.user_email = NEW.user_email
        AND organization_invites.invite_status = 'accepted'
    ) >= 1 THEN
        RAISE EXCEPTION 'Cannot invite a user to an organization they are already a member of.';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- migrate:down
-- Revert to original (buggy) version is not safe — leave the fix in place
