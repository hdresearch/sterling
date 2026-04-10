-- migrate:up
-- Remove the UNIQUE constraint on accounts.billing_email
-- This allows the same person to be the billing contact for multiple organizations
ALTER TABLE accounts DROP CONSTRAINT IF EXISTS accounts_billing_email_key;

-- migrate:down
ALTER TABLE accounts ADD CONSTRAINT accounts_billing_email_key UNIQUE (billing_email);
