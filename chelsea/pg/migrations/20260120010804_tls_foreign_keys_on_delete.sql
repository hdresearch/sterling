-- migrate:up

-- Add ON DELETE SET NULL to foreign keys on domains table.
-- This ensures that when ACME challenges are cleaned up or certs are deleted,
-- the domain records remain but lose their references.

-- Drop existing foreign key constraints (they were created without ON DELETE behavior)
ALTER TABLE domains
    DROP CONSTRAINT IF EXISTS domains_acme_http01_challenge_domain_fkey;

ALTER TABLE domains
    DROP CONSTRAINT IF EXISTS domains_tls_cert_id_fkey;

-- Recreate with ON DELETE SET NULL
-- When an ACME challenge is deleted (after cert issuance), domain keeps existing
ALTER TABLE domains
    ADD CONSTRAINT domains_acme_http01_challenge_domain_fkey
    FOREIGN KEY (acme_http01_challenge_domain)
    REFERENCES acme_http01_challenges(domain)
    ON DELETE SET NULL;

-- When a TLS cert is deleted (expired, revoked, replaced), domain keeps existing
ALTER TABLE domains
    ADD CONSTRAINT domains_tls_cert_id_fkey
    FOREIGN KEY (tls_cert_id)
    REFERENCES tls_certs(id)
    ON DELETE SET NULL;

-- migrate:down

-- Revert to original constraints without ON DELETE behavior
ALTER TABLE domains
    DROP CONSTRAINT IF EXISTS domains_acme_http01_challenge_domain_fkey;

ALTER TABLE domains
    DROP CONSTRAINT IF EXISTS domains_tls_cert_id_fkey;

ALTER TABLE domains
    ADD CONSTRAINT domains_acme_http01_challenge_domain_fkey
    FOREIGN KEY (acme_http01_challenge_domain)
    REFERENCES acme_http01_challenges(domain);

ALTER TABLE domains
    ADD CONSTRAINT domains_tls_cert_id_fkey
    FOREIGN KEY (tls_cert_id)
    REFERENCES tls_certs(id);
