-- migrate:up

CREATE EXTENSION IF NOT EXISTS pgcrypto; -- UUID generation
CREATE EXTENSION IF NOT EXISTS citext; -- case‑insensitive text

CREATE TABLE users (
    user_id     UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    oauth_provider_user_id TEXT UNIQUE, -- An ID that OAuth providers (e.g. GitHub) use to reference users. Used for account lookup.
    email       CITEXT      NOT NULL UNIQUE, -- matches billing_email for owner
    email_verified BOOLEAN  NOT NULL DEFAULT FALSE,
    email_verification_nonce UUID,
    email_verification_nonce_expires_at TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '10 minutes'),
    user_name   CITEXT      NOT NULL UNIQUE, -- e.g. "mistachkin_1234"
    avatar_uri  TEXT,
    passwd_algo TEXT,       -- algorithm name, e.g. PBKDF2, etc.
    passwd_iter INTEGER,    -- hash iteration count, e.g. 7777801
    passwd_salt TEXT,       -- salt for one-way hash
    passwd_hash TEXT,       -- salted one-way hash
    is_human    BOOLEAN     NOT NULL DEFAULT TRUE,
    is_active   BOOLEAN     NOT NULL DEFAULT TRUE,
    is_deleted  BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

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

CREATE INDEX IF NOT EXISTS idx_users_email_verification_nonce ON users(email_verification_nonce);

CREATE TABLE IF NOT EXISTS oauth_user_profiles (
    oauth_provider TEXT NOT NULL,
    oauth_provider_user_id TEXT NOT NULL,
    email CITEXT,
    user_name TEXT,
    name TEXT
);

CREATE TABLE accounts (
    account_id     UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name           CITEXT      NOT NULL,
    billing_email  CITEXT      NOT NULL UNIQUE REFERENCES users(email) ON DELETE NO ACTION,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ,
    CHECK (expires_at IS NULL OR expires_at >= created_at)
);

CREATE TABLE organizations (
    org_id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id     UUID        NOT NULL REFERENCES accounts(account_id) ON DELETE NO ACTION,
    parent_org_id  UUID            REFERENCES organizations(org_id)     ON DELETE NO ACTION,
    name           CITEXT      NOT NULL,
    description    TEXT,
    avatar_uri     CITEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_deleted     BOOLEAN DEFAULT FALSE,
    CHECK (parent_org_id <> org_id)
);

CREATE TABLE IF NOT EXISTS organization_invites (
    invite_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(org_id) ON DELETE NO ACTION,
    user_email CITEXT NOT NULL,
    invite_status TEXT NOT NULL CHECK(invite_status IN ('pending', 'accepted', 'rejected')),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '14 days')
);

CREATE INDEX IF NOT EXISTS idx_org_invites_org_user_email
    ON organization_invites(org_id, user_email);
CREATE INDEX IF NOT EXISTS idx_organization_invites_status
    ON organization_invites(invite_status);
CREATE INDEX IF NOT EXISTS idx_organization_invites_expires_at
    ON organization_invites(expires_at);
CREATE INDEX IF NOT EXISTS organizations_idx_1
    ON organizations(parent_org_id);

CREATE TABLE user_org_memberships (
    id         BIGSERIAL PRIMARY KEY,
    org_id     UUID NOT NULL REFERENCES organizations(org_id) ON DELETE NO ACTION,
    user_id    UUID NOT NULL REFERENCES users(user_id)        ON DELETE NO ACTION,
    is_active   BOOLEAN    NOT NULL DEFAULT TRUE,
    is_deleted  BOOLEAN    NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    UNIQUE (org_id, user_id),
    CHECK (expires_at IS NULL OR expires_at >= created_at),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX IF NOT EXISTS user_org_memberships_idx_1
    ON user_org_memberships(user_id);

CREATE TABLE permissions (
    permission_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          CITEXT NOT NULL UNIQUE,
    description   TEXT
);

CREATE TABLE api_keys (
    api_key_id             UUID PRIMARY KEY DEFAULT gen_random_uuid(), -- not secret
    user_id                UUID NOT NULL REFERENCES users(user_id) ON DELETE NO ACTION,
    org_id                 UUID NOT NULL REFERENCES organizations(org_id) ON DELETE NO ACTION,
    label    CITEXT        NOT NULL, -- user chosen nickname, e.g. 'default'
    key_algo TEXT          NOT NULL, -- algorithm name, e.g. PBKDF2, etc.
    key_iter INTEGER       NOT NULL, -- hash iteration count, e.g. 7777801
    key_salt TEXT          NOT NULL, -- salt for one-way hash
    key_hash TEXT          NOT NULL UNIQUE, -- salted one-way hash
    is_active BOOLEAN      NOT NULL DEFAULT TRUE,
    is_deleted BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    CHECK   (expires_at IS NULL OR expires_at >= created_at),
    CHECK   (revoked_at IS NULL OR revoked_at >= created_at),
    CHECK   (deleted_at IS NULL OR deleted_at >= created_at)
);

CREATE TABLE api_key_permissions (
    id            BIGSERIAL PRIMARY KEY,
    api_key_id    UUID NOT NULL REFERENCES api_keys(api_key_id)       ON DELETE NO ACTION,
    permission_id UUID NOT NULL REFERENCES permissions(permission_id) ON DELETE NO ACTION,
    is_active     BOOLEAN NOT NULL DEFAULT TRUE,
    is_deleted    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ,
    deleted_at    TIMESTAMPTZ,
    UNIQUE (api_key_id, permission_id),
    CHECK   (expires_at IS NULL OR expires_at >= created_at),
    CHECK   (revoked_at IS NULL OR revoked_at >= created_at),
    CHECK   (deleted_at IS NULL OR deleted_at >= created_at)
);

CREATE TABLE usage_data (
    usage_data_id  BIGSERIAL PRIMARY KEY,
    api_key_id     UUID NOT NULL REFERENCES api_keys(api_key_id) ON DELETE NO ACTION,
    node_id        UUID NOT NULL,
    vm_id          UUID NOT NULL,
    recorded_hour  TIMESTAMPTZ NOT NULL,
    cpu_usage      NUMERIC(12,4) NOT NULL CHECK (cpu_usage >= 0),
    cpu_units      TEXT NOT NULL,
    mem_usage      NUMERIC(12,4) NOT NULL CHECK (mem_usage >= 0),
    mem_units      TEXT NOT NULL,
    xfer_usage     NUMERIC(12,4) NOT NULL CHECK (xfer_usage >= 0),
    xfer_units     TEXT NOT NULL,
    storage_usage  NUMERIC(12,4) NOT NULL CHECK (storage_usage >= 0),
    storage_units  TEXT NOT NULL,
    UNIQUE (api_key_id, node_id, vm_id, recorded_hour),
    CHECK (recorded_hour = date_trunc('hour', recorded_hour))
);

CREATE TABLE email_signups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    email      TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
);

CREATE TABLE nodes (
    node_id        UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    ip             INET NOT NULL,
    status         CHARACTER varying(20) DEFAULT 'inactive'::character varying NOT NULL,
    cpu            INT NOT NULL,
    mem_gb         INT NOT NULL,
    disk_gb        INT NOT NULL,
    provider       TEXT NOT NULL,
    last_heartbeat TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    created_at     TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    CONSTRAINT nodes_status_check CHECK (((status)::text = ANY (ARRAY[('active'::character varying)::text, ('inactive'::character varying)::text, ('maintenance'::character varying)::text])))
);

CREATE TABLE clusters (
    cluster_id    UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    owner_id      UUID NOT NULL REFERENCES api_keys(api_key_id),
    max_concurrent_branches INT DEFAULT 10 NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
);

CREATE TABLE vms (
    vm_id          UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    cluster_id     UUID NOT NULL REFERENCES clusters(cluster_id),
    node_id        UUID NOT NULL REFERENCES nodes(node_id),
    ip             INET NOT NULL,
    wg_private_key TEXT NOT NULL,
    wg_public_key  TEXT NOT NULL,
    parent         UUID REFERENCES vms(vm_id),
    created_at     TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
);

CREATE TABLE domains (
    domain_id         BIGSERIAL PRIMARY KEY,
    owner_id   UUID NOT NULL REFERENCES users(user_id), -- is users right here?
    vm_id      UUID NOT NULL REFERENCES vms(vm_id),
    domain     TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
);

CREATE TABLE rootfs (
    rootfs_id     UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    cluster_id    UUID NOT NULL REFERENCES clusters(cluster_id),
    name          TEXT NOT NULL UNIQUE,
    description   TEXT,
    created_at    TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
);

CREATE TABLE commits (
    commit_id    UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    owner_id      UUID NOT NULL REFERENCES api_keys(api_key_id),
    name          TEXT NOT NULL UNIQUE,
    description   TEXT,
    created_at    TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
);

CREATE TABLE kernel (
    kernel_id     UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    cluster_id    UUID NOT NULL REFERENCES clusters(cluster_id),
    name          TEXT NOT NULL UNIQUE,
    description   TEXT,
    created_at    TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now()
);


CREATE OR REPLACE FUNCTION organization_guard_integrity()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    cycle_detected BOOLEAN;
    root_ancestor_id TEXT;
    root_ancestor_name TEXT;
    org_subtree_contains_org_with_same_name BOOLEAN := FALSE;
BEGIN
    /* If no parent, check that the org name is unique among parentless orgs. */
    IF NEW.parent_org_id IS NULL THEN
        IF EXISTS (
            SELECT 1 FROM organizations o
            WHERE o.parent_org_id IS NULL
            AND o.name = NEW.name
            AND o.org_id != NEW.org_id
        ) THEN
            RAISE EXCEPTION
                'Organization without a parent must have a unique name'
            USING ERRCODE = '23505';
        END IF;
        RETURN NEW;
    END IF;

    /* 1) Parent must belong to the same account. */
    IF NEW.account_id IS DISTINCT FROM (
           SELECT account_id
             FROM organizations
            WHERE org_id = NEW.parent_org_id
       ) THEN
        RAISE EXCEPTION
            'Parent organization % must belong to the same account as child %',
            NEW.parent_org_id, NEW.org_id;
    END IF;

    /* 2) Prevent cycles and compute root ancestor in one pass. */
    WITH RECURSIVE ancestors AS (
        SELECT o.org_id, o.parent_org_id, o.name AS org_name
          FROM organizations o
         WHERE o.org_id = NEW.parent_org_id
        UNION ALL
        SELECT o.org_id, o.parent_org_id, o.name
          FROM organizations o
          JOIN ancestors a ON o.org_id = a.parent_org_id
    ),
    root AS (
        SELECT a.org_id::TEXT AS org_id, a.org_name
          FROM ancestors a
         WHERE a.parent_org_id IS NULL
         LIMIT 1
    )
    SELECT
        EXISTS (SELECT 1 FROM ancestors WHERE org_id = NEW.org_id) AS has_cycle,
        (SELECT org_id FROM root) AS root_id,
        (SELECT org_name FROM root) AS root_name
    INTO cycle_detected, root_ancestor_id, root_ancestor_name;

    IF cycle_detected THEN
        RAISE EXCEPTION
            'Hierarchy cycle detected: organization % would be its own ancestor',
            NEW.org_id;
    END IF;

    /* Check descendant of root ancestor with same name does not exist */
    SELECT
        find_descendant_org_by_name(root_ancestor_name, NEW.name) IS NOT NULL
        AND find_descendant_org_by_name(root_ancestor_name, NEW.name) != NEW.org_id
    INTO org_subtree_contains_org_with_same_name;

    IF org_subtree_contains_org_with_same_name THEN
        RAISE EXCEPTION
            'Organization name % must be unique within the subtree of root %',
            NEW.name, root_ancestor_name;
    END IF;

    RETURN NEW;
END;
$$;

CREATE CONSTRAINT TRIGGER trg_org_integrity
AFTER INSERT OR UPDATE OF parent_org_id, account_id
ON organizations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION organization_guard_integrity();

-- Create function to check and limit non-expired invites
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

    -- Check if the user has already accepted an
    -- invite for that org and they are not currently a member
    if (
        SELECT COUNT(*)
        FROM organization_invites
        JOIN users u ON u.email = NEW.user_email
        JOIN user_org_memberships um ON um.user_id = u.user_id
        WHERE organization_invites.org_id = NEW.org_id
        AND um.is_deleted = FALSE
        AND organization_invites.user_email = NEW.user_email
        AND organization_invites.invite_status = 'accepted'
    ) >= 1 THEN
        RAISE EXCEPTION 'Cannot invite a user
        to an organization they have accepted an invite for.';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger to enforce the limit
CREATE TRIGGER enforce_org_invites_integrity
    BEFORE INSERT ON organization_invites
    FOR EACH ROW
    EXECUTE FUNCTION check_org_invites_integrity();

CREATE OR REPLACE FUNCTION find_descendant_org_by_name(root_org_name TEXT, child_org_name TEXT)
RETURNS UUID
LANGUAGE plpgsql
STRICT
AS $$
DECLARE
    child_org_id UUID;
BEGIN

    -- child orgs must have unique names within an organization's hierarchy
    WITH RECURSIVE child_orgs AS (
        -- start with the root
        SELECT
          organizations.org_id AS org_id,
          organizations.name AS org_name
        FROM organizations
        WHERE organizations.name = root_org_name AND
        organizations.parent_org_id IS NULL
        UNION ALL
        -- Search for all child orgs
        SELECT
            o.org_id AS org_id,
            o.name AS org_name
        FROM organizations o
        JOIN child_orgs c ON c.org_id = o.parent_org_id
    )
    -- find the child org with the supplied name
    SELECT c.org_id
      INTO STRICT child_org_id
      FROM child_orgs c
     WHERE c.org_name = child_org_name
     LIMIT 1;

    RETURN child_org_id;
END;
$$;

CREATE OR REPLACE FUNCTION find_org_descendants(root_org_id UUID)
RETURNS TABLE (
    org_id         UUID,
    account_id     UUID ,
    parent_org_id  UUID,
    name           CITEXT,
    description    TEXT,
    avatar_uri     CITEXT,
    created_at     TIMESTAMPTZ,
    is_deleted     BOOLEAN
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
        organizations.*
    FROM child_orgs
    JOIN organizations ON organizations.org_id = child_orgs.org_id
    WHERE child_orgs.org_id != root_org_id;
END;
$$;

CREATE OR REPLACE FUNCTION
find_root_org_by_name(rootOrgName TEXT)
RETURNS TABLE (
    org_id         UUID,
    account_id     UUID,
    parent_org_id  UUID,
    name           CITEXT,
    description    TEXT,
    avatar_uri     CITEXT,
    created_at     TIMESTAMPTZ,
    is_deleted     BOOLEAN
)
LANGUAGE plpgsql
STRICT
AS $$

BEGIN
    RETURN QUERY
    SELECT * FROM organizations o
    WHERE
        o.name = rootOrgName AND
        o.parent_org_id IS NULL
    LIMIT 1;
END;
$$;


CREATE OR REPLACE FUNCTION fetch_pending_org_invites_for_user(invited_user_id UUID)
RETURNS TABLE (
    org_name CITEXT,
    org_avatar_uri CITEXT,
    parent_org_name CITEXT,
    parent_org_id UUID,
    invite_id UUID,
    org_id UUID,
    user_email CITEXT,
    invite_status TEXT,
    created_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
)
LANGUAGE plpgsql
STRICT
AS $$

BEGIN
    RETURN QUERY
    SELECT o.name AS org_name, o.avatar_uri AS org_avatar_uri, po.name AS parent_org_name, po.org_id AS parent_org_id, oi.*
    FROM organization_invites oi
    JOIN organizations o ON o.org_id = oi.org_id
    LEFT JOIN organizations po ON po.org_id = o.parent_org_id
    JOIN users u ON u.email = oi.user_email
    WHERE
        u.user_id = invited_user_id AND
        oi.expires_at > NOW() AND
        oi.invite_status = 'pending';
END;
$$;

-- Helper function for checking if api keys, api key permissions, etc. are valid
CREATE OR REPLACE FUNCTION is_resource_valid(
    is_deleted BOOLEAN,
    deleted_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ
) RETURNS BOOLEAN AS $$
BEGIN
    RETURN is_deleted = FALSE AND
        deleted_at IS NULL AND
        revoked_at IS NULL AND
        (expires_at IS NULL OR expires_at > NOW());
END;
$$ LANGUAGE plpgsql IMMUTABLE;


-- migrate:down


