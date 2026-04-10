
-- migrate:up
ALTER TABLE commits
    ADD COLUMN parent_vm_id uuid NULL REFERENCES vms(vm_id);
COMMENT ON COLUMN commits.parent_vm_id IS 'References the VM that this commit was created from, if any.';
ALTER TABLE commits
    ADD COLUMN grandparent_commit_id uuid NULL REFERENCES commits(commit_id);
COMMENT ON COLUMN commits.grandparent_commit_id IS 'References the grandparent commit (parent of the parent), if any, to optimize commit tree traversal.';

ALTER TABLE vms
    ADD COLUMN parent_commit_id uuid NULL REFERENCES commits(commit_id);
COMMENT ON COLUMN vms.parent_commit_id IS 'References the commit that this VM was started from, if any.';
ALTER TABLE vms
    ADD COLUMN grandparent_vm_id uuid NULL REFERENCES vms(vm_id);
COMMENT ON COLUMN vms.grandparent_vm_id IS 'References the grandparent VM (parent of the parent), if any, to optimize VM tree traversal.';

ALTER TABLE vms
    DROP COLUMN IF EXISTS parent;

-- migrate:down
ALTER TABLE commits
    DROP COLUMN IF EXISTS parent_vm_id;
ALTER TABLE commits
    DROP COLUMN IF EXISTS grandparent_commit_id;

ALTER TABLE vms
    DROP COLUMN IF EXISTS parent_commit_id;
ALTER TABLE vms
    DROP COLUMN IF EXISTS grandparent_vm_id;

ALTER TABLE vms
    ADD COLUMN parent uuid NULL REFERENCES vms(vm_id);
