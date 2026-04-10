-- migrate:up

-- DNS is case-insensitive per RFC 1035, so we enforce lowercase storage
-- and add case-insensitive unique constraints.

-- Add unique index on lowercase domain for case-insensitive uniqueness
-- This prevents storing both "Example.com" and "example.com"
CREATE UNIQUE INDEX IF NOT EXISTS domains_domain_lower_unique_idx
    ON domains (LOWER(domain));

-- Add check constraint to ensure domains are stored in lowercase
-- This enforces that all new inserts/updates use lowercase
ALTER TABLE domains
    ADD CONSTRAINT domains_domain_lowercase_check
    CHECK (domain = LOWER(domain));

-- Same for acme_http01_challenges table
-- Note: domain is the PRIMARY KEY, but we still need case-insensitivity
ALTER TABLE acme_http01_challenges
    ADD CONSTRAINT acme_http01_challenges_domain_lowercase_check
    CHECK (domain = LOWER(domain));

-- migrate:down

ALTER TABLE acme_http01_challenges
    DROP CONSTRAINT IF EXISTS acme_http01_challenges_domain_lowercase_check;

ALTER TABLE domains
    DROP CONSTRAINT IF EXISTS domains_domain_lowercase_check;

DROP INDEX IF EXISTS domains_domain_lower_unique_idx;
