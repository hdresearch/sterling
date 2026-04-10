-- migrate:up

-- Step 1: prefer the creator, then other admins, then earliest active member.
WITH ranked_members AS (
    SELECT
        uom.org_id,
        uom.user_id,
        ROW_NUMBER() OVER (
            PARTITION BY uom.org_id
            ORDER BY
                CASE
                    WHEN uom.joined_via = 'creator' THEN 0
                    WHEN uom.role = 'admin' THEN 1
                    ELSE 2
                END,
                uom.created_at
        ) AS rn
    FROM user_org_memberships uom
    WHERE uom.is_deleted = FALSE
      AND uom.is_active = TRUE
)
UPDATE organizations o
SET billing_contact_id = rm.user_id
FROM ranked_members rm
WHERE o.org_id = rm.org_id
  AND rm.rn = 1
  AND o.billing_contact_id IS NULL;

-- Step 2: fall back to the account billing contact (email) when still missing.
WITH account_contacts AS (
    SELECT
        o.org_id,
        u.user_id
    FROM organizations o
    JOIN accounts a ON a.account_id = o.account_id
    JOIN users u ON u.email = a.billing_email
    WHERE o.billing_contact_id IS NULL
)
UPDATE organizations o
SET billing_contact_id = ac.user_id
FROM account_contacts ac
WHERE o.org_id = ac.org_id
  AND o.billing_contact_id IS NULL;

-- Ensure all orgs now have a billing contact; abort otherwise.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM organizations WHERE billing_contact_id IS NULL) THEN
        RAISE EXCEPTION 'organizations rows still missing billing_contact_id after backfill';
    END IF;
END
$$;

ALTER TABLE organizations
    ALTER COLUMN billing_contact_id SET NOT NULL;

-- migrate:down
ALTER TABLE organizations
    ALTER COLUMN billing_contact_id DROP NOT NULL;
