-- migrate:up

-- Add is_public flag to commit_repositories
ALTER TABLE commit_repositories
    ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT FALSE;

-- Partial index for efficient public repo listing
CREATE INDEX idx_commit_repositories_is_public
    ON commit_repositories(is_public) WHERE is_public = TRUE;

COMMENT ON COLUMN commit_repositories.is_public IS 'When true, the repository and its tags are visible to all users (including unauthenticated). Write operations remain restricted to the owning org.';

-- migrate:down

DROP INDEX IF EXISTS idx_commit_repositories_is_public;
ALTER TABLE commit_repositories DROP COLUMN IF EXISTS is_public;
