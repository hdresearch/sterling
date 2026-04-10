-- migrate:up
CREATE TABLE acme_http01_challenges (
  domain TEXT PRIMARY KEY,
  -- /.well-known/acme-challenge/<this-token>
  challenge_token TEXT NOT NULL,
  -- What to serve back as body to the request (key authorization: <token>.<account_key>)
  challenge_value TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE tls_certs (
  id UUID PRIMARY KEY,
  cert_chain TEXT NOT NULL,
  cert_private_key TEXT NOT NULL,
  cert_not_after TIMESTAMPTZ NOT NULL,
  cert_not_before TIMESTAMPTZ NOT NULL,
  issued_at TIMESTAMPTZ NOT NULL
);

ALTER TABLE domains
  ADD COLUMN acme_http01_challenge_domain TEXT REFERENCES acme_http01_challenges(domain);

ALTER TABLE domains
  ADD COLUMN tls_cert_id UUID REFERENCES tls_certs(id);

-- migrate:down
ALTER TABLE domains DROP COLUMN IF EXISTS acme_http01_challenge_domain;
ALTER TABLE domains DROP COLUMN IF EXISTS tls_cert_id;

DROP TABLE IF EXISTS tls_certs;
DROP TABLE IF EXISTS acme_http01_challenges;
