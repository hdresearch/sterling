-- migrate:up

-- Add is_public column to commits table
ALTER TABLE commits ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT FALSE;

-- Partial index for efficiently querying public commits
CREATE INDEX idx_commits_is_public ON commits(is_public) WHERE is_public = TRUE;

-- migrate:down

DROP INDEX IF EXISTS idx_commits_is_public;
ALTER TABLE commits DROP COLUMN IF EXISTS is_public;
