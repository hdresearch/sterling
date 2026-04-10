-- migrate:up

-- Second user, account, org, and API key for cross-org integration tests.

INSERT INTO users (
  user_id, email, user_name, passwd_algo, passwd_iter, passwd_salt, passwd_hash
) VALUES (
  'a0a1a2a3-a4a5-a6a7-a8a9-aaabacadaeaf',  -- User Id
  'second@vers.sh',                          -- Email
  'second_user',                             -- Username
  'PBKDF2', 100,
  'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
);

INSERT INTO accounts (
  account_id, name, billing_email
) VALUES (
  'b8a1c2d3-e4f5-6789-abcd-ef0123456789',  -- ID
  'Second Test Account',                     -- Account name
  'second@vers.sh'                           -- Billing email (references users.email)
);

INSERT INTO organizations (
  org_id, account_id, name, description, billing_contact_id
) VALUES (
  'c9b2d3e4-f5a6-7890-bcde-f01234567890',  -- Org Id
  'b8a1c2d3-e4f5-6789-abcd-ef0123456789',  -- Account Id (second account)
  'second_org',                              -- Name
  'Second organization for cross-org tests', -- Description
  'a0a1a2a3-a4a5-a6a7-a8a9-aaabacadaeaf'   -- Billing contact (second user)
);

INSERT INTO api_keys (
  api_key_id, user_id, org_id, label, key_algo, key_iter, key_salt,
  key_hash
) VALUES (
  'a1b2c3d4-e5f6-7890-abcd-ef0123456789',  -- Key Id
  'a0a1a2a3-a4a5-a6a7-a8a9-aaabacadaeaf',  -- User Id (second user)
  'c9b2d3e4-f5a6-7890-bcde-f01234567890',  -- Org Id (second org)
  'Second Org Key',                          -- Label
  'PBKDF2',                                  -- Key Algorithm
  100,                                       -- Iterations
  'a1b2c3d4e5f6789012345678abcdef01',        -- Salt
  -- Key Hash (not real, tests use key entity directly not bearer auth for this key)
  'deadbeef01234567890abcdef01234567890abcdef01234567890abcdef01234567890abcdef01234567890abcdef01234567890abcdef01234567890abcdef0123456'
);

-- migrate:down

DELETE FROM api_keys WHERE api_key_id = 'a1b2c3d4-e5f6-7890-abcd-ef0123456789';
DELETE FROM organizations WHERE org_id = 'c9b2d3e4-f5a6-7890-bcde-f01234567890';
DELETE FROM accounts WHERE account_id = 'b8a1c2d3-e4f5-6789-abcd-ef0123456789';
DELETE FROM users WHERE user_id = 'a0a1a2a3-a4a5-a6a7-a8a9-aaabacadaeaf';
