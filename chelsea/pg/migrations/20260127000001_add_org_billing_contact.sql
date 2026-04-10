-- migrate:up

-- =============================================================================
-- Add billing_contact_id to organizations table
-- =============================================================================
--
-- Context: vers-landing PR #218 (SSO v2) introduces org-level billing where:
-- - Each organization has a subscription (stored in vers_landing.org_subscriptions)
-- - One member is designated as the "billing contact" who is responsible for payment
-- - The billing contact is typically the org owner, but can be reassigned
--
-- Why this column is in Chelsea (public.organizations) instead of vers_landing:
-- - Chelsea owns the organizations table and may need to know who pays for an org
-- - Keeps billing ownership info with the org, separate from subscription details
-- - vers_landing.org_subscriptions stores subscription details (tier, status, Flowglad IDs)
--   and references this column for the billing contact
--
-- Behavior:
-- - When org is created, billing_contact_id = creator (the first admin)
-- - Org owner can reassign billing contact to any org member
-- - When ownership transfers, billing contact automatically transfers to new owner
-- - NULL allowed for existing orgs until backfilled
--
-- Related changes in vers-landing:
-- - vers_landing.org_subscriptions table (org-level subscription data)
-- - change-billing-contact.ts action (owner can reassign)
-- - transfer-ownership.ts updates billing contact on ownership transfer
-- =============================================================================

ALTER TABLE organizations
  ADD COLUMN billing_contact_id UUID REFERENCES users(user_id) ON DELETE SET NULL;

-- Index for efficient lookups (e.g., "which orgs is this user billing contact for?")
CREATE INDEX idx_organizations_billing_contact ON organizations(billing_contact_id)
  WHERE billing_contact_id IS NOT NULL;

-- migrate:down
DROP INDEX IF EXISTS idx_organizations_billing_contact;
ALTER TABLE organizations DROP COLUMN IF EXISTS billing_contact_id;
