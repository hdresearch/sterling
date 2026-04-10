-- migrate:up

-- Fix next_vm_ip to return /128 when no VMs exist for the account
-- Previously would return subnet + 1 which inherited the subnet mask
-- Now explicitly returns as /128 (single host address)

CREATE OR REPLACE FUNCTION next_vm_ip(_account_id UUID)
  RETURNS INET
  LANGUAGE plpgsql
AS $$
DECLARE
  _subnet    INET;
  _ip        INET;
BEGIN
   SELECT network INTO _subnet FROM accounts WHERE account_id = _account_id;

   SELECT MAX(ip) INTO _ip
   FROM vms
   LEFT JOIN clusters ON vms.cluster_id = clusters.cluster_id
   LEFT JOIN api_keys ON clusters.owner_id = api_keys.api_key_id
   LEFT JOIN organizations ON api_keys.org_id = organizations.org_id
   LEFT JOIN accounts ON organizations.account_id = accounts.account_id
   WHERE accounts.account_id = _account_id;

   IF _ip IS NULL THEN
     RETURN set_masklen(_subnet + 1, 128);
   ELSE
     RETURN set_masklen(_ip + 1, 128);
   END IF;
END
$$;

-- migrate:down
