#!/bin/bash

set -eou pipefail

POSTGRES_PASSWORD=opensesame
POSTGRES_USER=postgres
POSTGRES_DB=vers
PG=postgresql://${POSTGRES_USER}:${POSTGRES_PASSWORD}@127.0.0.1:5432/${POSTGRES_DB}?sslmode=disable

# Generate the certificate
echo "[1/2] Generating certs..."
mkcert 'api.vers.sh' '*.vm.vers.sh'
mkcert -install

# Set permissions
# These are not really secret, and their default permissions cause errors in other
# scripts
chmod 777 api.vers.sh+1-key.pem api.vers.sh+1.pem

# Read the generated certificate files
CERT_KEY=$(cat api.vers.sh+1-key.pem)
CERT_CHAIN=$(cat api.vers.sh+1.pem)

# Get expiry dates from the certificate
CERT_NOT_AFTER=$(openssl x509 -in api.vers.sh+1.pem -noout -enddate | cut -d= -f2)
CERT_NOT_BEFORE=$(openssl x509 -in api.vers.sh+1.pem -noout -startdate | cut -d= -f2)

# Convert to PostgreSQL timestamp format
CERT_NOT_AFTER_PG=$(date -d "$CERT_NOT_AFTER" "+%Y-%m-%d %H:%M:%S+00")
CERT_NOT_BEFORE_PG=$(date -d "$CERT_NOT_BEFORE" "+%Y-%m-%d %H:%M:%S+00")

# Insert into database
echo "[2/2] Inserting generated certs..."
psql $PG > /dev/null <<SQL
INSERT INTO tls_certs (id, cert_private_key, cert_chain, cert_not_after, cert_not_before, issued_at)
VALUES (
  'b0e4346b-302e-49c4-9692-4dbfdf8b2cbc',
  '$CERT_KEY',
  '$CERT_CHAIN',
  '$CERT_NOT_AFTER_PG',
  '$CERT_NOT_BEFORE_PG',
  now()
)
ON CONFLICT (id) DO UPDATE SET
  cert_private_key = EXCLUDED.cert_private_key,
  cert_chain = EXCLUDED.cert_chain,
  cert_not_after = EXCLUDED.cert_not_after,
  cert_not_before = EXCLUDED.cert_not_before,
  issued_at = EXCLUDED.issued_at;
SQL

echo "Done!"
