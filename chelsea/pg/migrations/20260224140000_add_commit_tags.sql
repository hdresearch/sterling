-- migrate:up

-- Create the commit_tags table for managing mutable pointers to commits
CREATE TABLE IF NOT EXISTS commit_tags (
    tag_id UUID PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
    tag_name TEXT NOT NULL,
    commit_id UUID NOT NULL REFERENCES commits(commit_id) ON DELETE CASCADE,
    owner_id UUID NOT NULL REFERENCES api_keys(api_key_id),
    org_id UUID NOT NULL REFERENCES organizations(org_id),
    description TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),

    -- Ensure tag names are unique per organization
    CONSTRAINT unique_tag_per_org UNIQUE (org_id, tag_name)
);

-- Add indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_commit_tags_org_id ON commit_tags(org_id);
CREATE INDEX IF NOT EXISTS idx_commit_tags_commit_id ON commit_tags(commit_id);
CREATE INDEX IF NOT EXISTS idx_commit_tags_owner_id ON commit_tags(owner_id);

-- Add helpful comments
COMMENT ON TABLE commit_tags IS 'Tags are mutable pointers to commits, scoped to organizations. Multiple tags can point to the same commit.';
COMMENT ON COLUMN commit_tags.owner_id IS 'The API key that created this tag. Used for audit trail.';
COMMENT ON COLUMN commit_tags.org_id IS 'The organization that owns this tag. Tags are organization-wide and accessible by any API key in the org.';
COMMENT ON COLUMN commit_tags.commit_id IS 'The commit this tag currently points to. CASCADE deletes tags when commit is deleted.';

-- migrate:down

DROP TABLE IF EXISTS commit_tags;
