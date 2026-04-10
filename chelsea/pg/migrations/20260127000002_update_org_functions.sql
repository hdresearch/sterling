-- migrate:up

-- =============================================================================
-- Update find_org_descendants function to include billing_contact_id
-- =============================================================================
-- The organizations table now has billing_contact_id column, so we need to update
-- this function to include it in the return type and select explicit columns
-- instead of using SELECT * which breaks when new columns are added.

DROP FUNCTION IF EXISTS find_org_descendants(UUID);

CREATE OR REPLACE FUNCTION find_org_descendants(root_org_id UUID)
RETURNS TABLE (
    org_id             UUID,
    account_id         UUID,
    parent_org_id      UUID,
    name               CITEXT,
    description        TEXT,
    avatar_uri         CITEXT,
    created_at         TIMESTAMPTZ,
    is_deleted         BOOLEAN,
    billing_contact_id UUID
)
STRICT
LANGUAGE plpgsql
AS $$

BEGIN
    RETURN QUERY
    WITH RECURSIVE child_orgs AS (
        -- start with the root
        SELECT
        organizations.org_id AS org_id,
        organizations.name AS org_name
        FROM organizations
        WHERE organizations.org_id = root_org_id
        UNION ALL
        -- Search for all child orgs
        SELECT
            o.org_id as org_id,
            o.name as org_name
        FROM organizations o
        JOIN child_orgs c ON c.org_id = o.parent_org_id
    )
    SELECT
        organizations.org_id,
        organizations.account_id,
        organizations.parent_org_id,
        organizations.name,
        organizations.description,
        organizations.avatar_uri,
        organizations.created_at,
        organizations.is_deleted,
        organizations.billing_contact_id
    FROM child_orgs
    JOIN organizations ON organizations.org_id = child_orgs.org_id
    WHERE child_orgs.org_id != root_org_id;
END;
$$;

-- =============================================================================
-- Update find_root_org_by_name function to include billing_contact_id
-- =============================================================================

DROP FUNCTION IF EXISTS find_root_org_by_name(TEXT);

CREATE OR REPLACE FUNCTION find_root_org_by_name(rootOrgName TEXT)
RETURNS TABLE (
    org_id             UUID,
    account_id         UUID,
    parent_org_id      UUID,
    name               CITEXT,
    description        TEXT,
    avatar_uri         CITEXT,
    created_at         TIMESTAMPTZ,
    is_deleted         BOOLEAN,
    billing_contact_id UUID
)
STRICT
LANGUAGE plpgsql
AS $$

BEGIN
    RETURN QUERY
    SELECT
        organizations.org_id,
        organizations.account_id,
        organizations.parent_org_id,
        organizations.name,
        organizations.description,
        organizations.avatar_uri,
        organizations.created_at,
        organizations.is_deleted,
        organizations.billing_contact_id
    FROM organizations
    WHERE organizations.name = rootOrgName::CITEXT AND organizations.parent_org_id IS NULL
    LIMIT 1;
END;
$$;

-- migrate:down
-- Note: Rolling back would require restoring the old function signatures
-- which is complex. In practice, we'd create a new forward migration instead.
