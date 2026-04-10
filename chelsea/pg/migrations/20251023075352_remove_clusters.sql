-- migrate:up

ALTER TABLE vms DROP COLUMN cluster_id;
ALTER TABLE rootfs DROP COLUMN cluster_id;
ALTER TABLE kernel DROP COLUMN cluster_id;
DROP TABLE clusters;

DELETE FROM vms;

ALTER TABLE vms
  ADD COLUMN owner_id UUID NOT NULL REFERENCES api_keys(api_key_id);

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
   LEFT JOIN api_keys ON vms.owner_id = api_keys.api_key_id
   LEFT JOIN organizations ON api_keys.org_id = organizations.org_id
   LEFT JOIN accounts ON organizations.account_id = accounts.account_id
   WHERE accounts.account_id = _account_id;

   IF _ip IS NULL THEN
     RETURN set_masklen(_subnet + 1, 64);
   ELSE
     RETURN set_masklen(_ip + 1, 64);
   END IF;
END
$$;

ALTER TABLE commits DROP COLUMN vm_id;

-- migrate:down
