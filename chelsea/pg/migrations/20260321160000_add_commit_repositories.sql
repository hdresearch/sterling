-- migrate:up

-- Create the commit_repositories table (named image groupings, like Docker repositories)
CREATE TABLE IF NOT EXISTS commit_repositories (
    repo_id     UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    org_id      UUID NOT NULL REFERENCES organizations(org_id) ON DELETE NO ACTION,
    name        TEXT NOT NULL,
    description TEXT,
    owner_id    UUID NOT NULL REFERENCES api_keys(api_key_id),
    created_at  TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),

    -- Repository names must be unique within an organization
    CONSTRAINT unique_repo_per_org UNIQUE (org_id, name)
);

CREATE INDEX IF NOT EXISTS idx_commit_repositories_org_id ON commit_repositories(org_id);
CREATE INDEX IF NOT EXISTS idx_commit_repositories_owner_id ON commit_repositories(owner_id);

COMMENT ON TABLE commit_repositories IS 'Named image repositories, scoped to organizations. Each repository groups related commits under a name (like Docker image names).';
COMMENT ON COLUMN commit_repositories.name IS 'The repository name (e.g. "myapp", "base-ubuntu"). Must be unique within an organization.';
COMMENT ON COLUMN commit_repositories.owner_id IS 'The API key that created this repository.';

-- Add repo_id column to commit_tags (nullable for backwards compatibility during migration)
ALTER TABLE commit_tags
    ADD COLUMN repo_id UUID REFERENCES commit_repositories(repo_id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_commit_tags_repo_id ON commit_tags(repo_id);

-- Replace the old blanket unique constraint with two partial unique indexes:
-- 1. Legacy org-scoped tags (repo_id IS NULL): unique by (org_id, tag_name)
-- 2. Repo-scoped tags (repo_id IS NOT NULL): unique by (repo_id, tag_name)
-- This allows the same tag_name in different repos within the same org.
ALTER TABLE commit_tags
    DROP CONSTRAINT unique_tag_per_org;

CREATE UNIQUE INDEX unique_tag_per_org_legacy
    ON commit_tags (org_id, tag_name)
    WHERE repo_id IS NULL;

CREATE UNIQUE INDEX unique_tag_per_repo
    ON commit_tags (repo_id, tag_name)
    WHERE repo_id IS NOT NULL;

COMMENT ON COLUMN commit_tags.repo_id IS 'Optional repository this tag belongs to. NULL for legacy org-scoped tags. When set, tag is scoped to the repository.';

-- migrate:down

DROP INDEX IF EXISTS unique_tag_per_repo;
DROP INDEX IF EXISTS unique_tag_per_org_legacy;

-- Restore the original blanket constraint
ALTER TABLE commit_tags
    ADD CONSTRAINT unique_tag_per_org UNIQUE (org_id, tag_name);

ALTER TABLE commit_tags
    DROP COLUMN IF EXISTS repo_id;

DROP INDEX IF EXISTS idx_commit_tags_repo_id;
DROP INDEX IF EXISTS idx_commit_repositories_org_id;
DROP INDEX IF EXISTS idx_commit_repositories_owner_id;

DROP TABLE IF EXISTS commit_repositories;
