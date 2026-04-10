-- migrate:up

ALTER TABLE accounts ADD COLUMN network INET UNIQUE;

CREATE OR REPLACE FUNCTION trg_account_network_assignment()
  RETURNS trigger
  LANGUAGE plpgsql
AS $$
BEGIN
   IF NEW.network IS NULL THEN
     NEW.network := (select (broadcast(coalesce(max(network),
                                       'fd00:fe11:deed::/64'::inet))
                     + 1) as network from accounts);
   END IF;
   RETURN NEW;
END
$$;

CREATE TRIGGER account_network_assignment
BEFORE INSERT ON accounts
FOR EACH ROW EXECUTE FUNCTION trg_account_network_assignment();


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

   RETURN COALESCE(_ip, _subnet) + 1;
END
$$;

-- migrate:down
