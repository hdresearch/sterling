#!/bin/bash

set -eou pipefail

# Script to insert a test custom domain for a VM to test proxy TLS functionality

POSTGRES_PASSWORD=opensesame
POSTGRES_USER=postgres
POSTGRES_DB=vers
PG=postgresql://${POSTGRES_USER}:${POSTGRES_PASSWORD}@127.0.0.1:5432/${POSTGRES_DB}?sslmode=disable

# Validation functions
validate_domain() {
    local domain="$1"

    # Check if domain is empty
    if [ -z "$domain" ]; then
        echo "Error: Domain cannot be empty" >&2
        return 1
    fi

    # Check for valid domain format (letters, numbers, dots, hyphens)
    # Must not start or end with dot or hyphen
    # Must have at least one dot for TLD
    if ! echo "$domain" | grep -qE '^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?)*\.[a-zA-Z]{2,}$'; then
        echo "Error: Invalid domain format: $domain" >&2
        echo "Domain must:" >&2
        echo "  - Contain only letters, numbers, dots, and hyphens" >&2
        echo "  - Not start or end with a dot or hyphen" >&2
        echo "  - Have a valid TLD (e.g., .com, .org)" >&2
        echo "Examples: example.com, sub.example.org, my-site.co.uk" >&2
        return 1
    fi

    # Check length constraints
    if [ ${#domain} -gt 253 ]; then
        echo "Error: Domain too long (max 253 characters): $domain" >&2
        return 1
    fi

    return 0
}

validate_uuid() {
    local uuid="$1"
    local name="$2"

    # Check if UUID is empty
    if [ -z "$uuid" ]; then
        echo "Error: $name cannot be empty" >&2
        return 1
    fi

    # Check for valid UUID format (8-4-4-4-12 hex digits)
    if ! echo "$uuid" | grep -qiE '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'; then
        echo "Error: Invalid $name format: $uuid" >&2
        echo "$name must be a valid UUID (format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)" >&2
        echo "Example: 550e8400-e29b-41d4-a716-446655440000" >&2
        return 1
    fi

    return 0
}

if [ -z "${1:-}" ] || [ -z "${2:-}" ]; then
    echo "Usage: $0 <domain> <vm_id> [tls_cert_id]"
    echo ""
    echo "Examples:"
    echo "  # Add domain without certificate (will trigger ACME)"
    echo "  $0 example.com 550e8400-e29b-41d4-a716-446655440000"
    echo ""
    echo "  # Add domain with existing certificate"
    echo "  $0 example.com 550e8400-e29b-41d4-a716-446655440000 b0e4346b-302e-49c4-9692-4dbfdf8b2cbc"
    exit 1
fi

DOMAIN="$1"
VM_ID="$2"
TLS_CERT_ID="${3:-}"

# Validate inputs
echo "Validating inputs..."

if ! validate_domain "$DOMAIN"; then
    exit 1
fi

if ! validate_uuid "$VM_ID" "VM ID"; then
    exit 1
fi

if [ -n "$TLS_CERT_ID" ]; then
    if ! validate_uuid "$TLS_CERT_ID" "TLS Cert ID"; then
        exit 1
    fi
fi

echo "✓ All inputs valid"
echo ""

echo "Inserting test domain into database..."
echo "  Domain: $DOMAIN"
echo "  VM ID: $VM_ID"
echo "  TLS Cert ID: ${TLS_CERT_ID:-<none - will trigger ACME>}"
echo ""

# Get owner_id from users table
OWNER_ID=$(psql "$PG" -t -c "SELECT user_id FROM users LIMIT 1;" 2>/dev/null | xargs || echo "")

if [ -z "$OWNER_ID" ]; then
    echo "Error: No users found in database. Cannot determine owner_id." >&2
    echo "Please ensure at least one user exists in the users table." >&2
    exit 1
fi

echo "Using owner_id: $OWNER_ID"
echo ""

# Check if domain already exists
EXISTING=$(psql "$PG" -t -c "SELECT domain FROM domains WHERE domain = '$DOMAIN';" 2>/dev/null | xargs || echo "")

if [ -n "$EXISTING" ]; then
    echo "Domain already exists. Updating..."

    if [ -n "$TLS_CERT_ID" ]; then
        # Update with certificate
        psql "$PG" -c "UPDATE domains SET vm_id = '$VM_ID', tls_cert_id = '$TLS_CERT_ID', owner_id = '$OWNER_ID' WHERE domain = '$DOMAIN';" || {
            echo "" >&2
            echo "Error: Failed to update domain in database" >&2
            exit 1
        }
    else
        # Update without certificate
        psql "$PG" -c "UPDATE domains SET vm_id = '$VM_ID', tls_cert_id = NULL, owner_id = '$OWNER_ID' WHERE domain = '$DOMAIN';" || {
            echo "" >&2
            echo "Error: Failed to update domain in database" >&2
            exit 1
        }
    fi

    echo "✓ Domain updated successfully!"
else
    echo "Inserting new domain..."

    if [ -n "$TLS_CERT_ID" ]; then
        # Insert with certificate
        psql "$PG" -c "INSERT INTO domains (domain, vm_id, tls_cert_id, owner_id) VALUES ('$DOMAIN', '$VM_ID', '$TLS_CERT_ID', '$OWNER_ID');" || {
            echo "" >&2
            echo "Error: Failed to insert domain into database" >&2
            exit 1
        }
    else
        # Insert without certificate
        psql "$PG" -c "INSERT INTO domains (domain, vm_id, tls_cert_id, owner_id) VALUES ('$DOMAIN', '$VM_ID', NULL, '$OWNER_ID');" || {
            echo "" >&2
            echo "Error: Failed to insert domain into database" >&2
            exit 1
        }
    fi

    echo "✓ Domain inserted successfully!"
fi

echo ""
echo "To test the domain routing:"
echo "  ./public-api.sh vm-request-custom $DOMAIN"
echo "  ./public-api.sh vm-request-custom $DOMAIN /health"
