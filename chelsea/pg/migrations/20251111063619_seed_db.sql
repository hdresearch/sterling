-- migrate:up

INSERT INTO users (
  user_id, email, user_name, passwd_algo, passwd_iter, passwd_salt, passwd_hash
) VALUES (
  '9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9',  -- User Id
  'test@vers.sh',                          -- Email
  'test_user',                             -- User naem
  'PBKDF2',                                -- Passwd Algo
  100,                                     -- Iter
  'f14efc4400f9f72b19b904a298638636',      -- Salt
  -- Hash
  '791057f4f3fb75547ac38b6fa4bf162009d88dcfc701ae52e1dbb7db28a6eb35d304675eaf49807406d3dea01a1cb55762b499853dbebbb2abfa23a7f58d358b'
);

INSERT INTO accounts (
  account_id, name, billing_email
) VALUES (
 '47750c3a-d1fa-4f33-8135-f972fadfe3bd', -- ID
 'Test Users Account',                   -- Account name
 'test@vers.sh'                          -- Billing email
);

INSERT INTO organizations (
  org_id, account_id, name, description
) VALUES (
  '2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d',  -- Org Id
  '47750c3a-d1fa-4f33-8135-f972fadfe3bd',  -- Account Id
  'test_user',                             -- Name
  'Default organization for Test User'     -- Description
);

INSERT INTO api_keys (
  api_key_id, user_id, org_id, label, key_algo, key_iter, key_salt,
  key_hash
) VALUES (
  'ef90fd52-66b5-47e7-b7dc-e73c4381028f',  -- Key Id
  '9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9',  -- User Id
  '2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d',  -- Org Id
  'Test Key',                              -- Label
  'PBKDF2',                                -- Key Algorithm
  100,                                     -- Iterations
  '1d509419920e52ffc5073e89fa712fdb',      -- Salt
  -- Key Hash
  -- Raw key
  -- ef90fd52-66b5-47e7-b7dc-e73c4381028fbfa85827e1f1ebab3078c3d3249a72647aef57451bd5feac7b727dcb5842590c
  '4c3bf6d11ca3cc96e69df912489f9c0ce8b74a4bc8e816342380f7cc2e4a5605776c4959e94bdad3205cbc71c8b5a6cc9f4d1eadf5cb8b03e77a182b7fe5cb9b'
);

INSERT INTO orchestrators (
  id, region, wg_public_key, wg_private_key, wg_ipv6, ip
) VALUES (
  '18e1ecdb-6e6c-4336-868b-29f42f25ea54',  -- ID
  'us-east',                               -- Region
  '2nwuaeo/vPD5FBmv+xXlvW8TR/qGurjzC+M0YVSaO28=', -- WG Pub key
  'GBPPRay4ZykbUy+O92D+G6kkgVVTiPEHCRvCBa+AnVA=', -- WG Pri Key
  'fd00:fe11:deed:0::ffff',                -- Orchestrator IPv6
  '127.0.0.2'                              -- Orchestrator IPv4
);

-- migrate:down

