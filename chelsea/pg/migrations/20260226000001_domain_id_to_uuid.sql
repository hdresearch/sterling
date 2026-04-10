-- migrate:up

-- Change domain_id from BIGSERIAL to UUID for consistency with other tables
-- First, add a new UUID column
ALTER TABLE domains ADD COLUMN new_domain_id UUID DEFAULT gen_random_uuid();

-- Update existing rows to have UUIDs
UPDATE domains SET new_domain_id = gen_random_uuid() WHERE new_domain_id IS NULL;

-- Make it NOT NULL
ALTER TABLE domains ALTER COLUMN new_domain_id SET NOT NULL;

-- Drop the old column and rename
ALTER TABLE domains DROP COLUMN domain_id;
ALTER TABLE domains RENAME COLUMN new_domain_id TO domain_id;

-- Add primary key constraint
ALTER TABLE domains ADD PRIMARY KEY (domain_id);

-- Fix owner_id foreign key: should reference api_keys like other tables, not users
-- The original schema had: owner_id REFERENCES users(user_id)
-- But the code passes api_key.id() which is api_key_id, matching other tables
ALTER TABLE domains
    DROP CONSTRAINT domains_owner_id_fkey;

ALTER TABLE domains
    ADD CONSTRAINT domains_owner_id_fkey
    FOREIGN KEY (owner_id) REFERENCES api_keys(api_key_id);

-- migrate:down

-- Revert owner_id foreign key back to users
ALTER TABLE domains
    DROP CONSTRAINT IF EXISTS domains_owner_id_fkey;

ALTER TABLE domains
    ADD CONSTRAINT domains_owner_id_fkey
    FOREIGN KEY (owner_id) REFERENCES users(user_id);

-- This is destructive - cannot restore original BIGSERIAL values
ALTER TABLE domains DROP CONSTRAINT domains_pkey;
ALTER TABLE domains ADD COLUMN old_domain_id BIGSERIAL;
ALTER TABLE domains DROP COLUMN domain_id;
ALTER TABLE domains RENAME COLUMN old_domain_id TO domain_id;
ALTER TABLE domains ADD PRIMARY KEY (domain_id);
