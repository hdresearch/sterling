#!/bin/bash
set -euo pipefail

# Certificate Setup Script
# Generates self-signed certificates for development or sets up Let's Encrypt for production

# Whether or not to overwrite the cert+key pem file, if they exist
overwrite=unset

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default values
DOMAIN="${DOMAIN:-*.vm.vers.sh}"
SSH_CERT_PATH="${SSH_CERT_PATH:-/etc/ssl/chelsea/proxy-cert.pem}"
SSH_KEY_PATH="${SSH_KEY_PATH:-/etc/ssl/chelsea/proxy-key.pem}"
MODE="${MODE:-dev}"

usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -m, --mode MODE        Mode: 'dev' (self-signed) or 'prod' (Let's Encrypt) [default: dev]"
    echo "  -d, --domain DOMAIN    Domain name [default: *.vm.vers.sh]"
    echo "  -c, --cert PATH        Certificate output path [default: /etc/ssl/chelsea/proxy-cert.pem]"
    echo "  -k, --key PATH         Key output path [default: /etc/ssl/chelsea/proxy-key.pem]"
    echo "  -e, --email EMAIL      Email for Let's Encrypt (required for prod mode)"
    echo "  --overwrite-certs      Overwrite current cert+key pem file, if they exist"
    echo "  --no-overwrite-certs   Preserve current cert+key pem file, if they exist"
    echo "  -h, --help             Show this help message"
    echo ""
    echo "Examples:"
    echo "  # Generate self-signed cert for development"
    echo "  $0 --mode dev --domain '*.vm.vers.sh'"
    echo ""
    echo "  # Set up Let's Encrypt for production"
    echo "  $0 --mode prod --domain vm.vers.sh --email admin@example.com"
    echo ""
    echo "Environment variables:"
    echo "  MODE        - Same as --mode"
    echo "  DOMAIN      - Same as --domain"
    echo "  SSH_CERT_PATH   - Same as --cert"
    echo "  SSH_KEY_PATH    - Same as --key"
    echo "  LETSENCRYPT_EMAIL - Email for Let's Encrypt"
    exit 0
}

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -m|--mode)
            MODE="$2"
            shift 2
            ;;
        -d|--domain)
            DOMAIN="$2"
            shift 2
            ;;
        -c|--cert)
            SSH_CERT_PATH="$2"
            shift 2
            ;;
        -k|--key)
            SSH_KEY_PATH="$2"
            shift 2
            ;;
        -e|--email)
            LETSENCRYPT_EMAIL="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        --overwrite-certs)
            overwrite=true
            shift 1
            ;;
        --no-overwrite-certs)
            overwrite=false
            shift 1
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            ;;
    esac
done

# Validate mode
if [[ "$MODE" != "dev" && "$MODE" != "prod" ]]; then
    log_error "Invalid mode: $MODE (must be 'dev' or 'prod')"
    exit 1
fi

generate_self_signed_cert() {
    local domain="$1"
    local cert_path="$2"
    local key_path="$3"

    log_info "Generating self-signed certificate for: $domain"

    # Create certificate directory if it doesn't exist
    mkdir -p "$(dirname "$cert_path")"
    mkdir -p "$(dirname "$key_path")"

    # Extract base domain for wildcard certs
    if [[ "$domain" == \*.* ]]; then
        base_domain="${domain#\*.}"
        san_entries="DNS:$domain,DNS:$base_domain"
        log_info "Wildcard certificate - including both $domain and $base_domain in SAN"
    else
        san_entries="DNS:$domain"
    fi

    # Generate private key
    openssl genrsa -out "$key_path" 2048 2>/dev/null

    # Create OpenSSL config for SAN
    local config_file=$(mktemp)
    cat > "$config_file" <<EOF
[req]
default_bits = 2048
prompt = no
default_md = sha256
distinguished_name = dn
req_extensions = v3_req

[dn]
C=US
O=Vers
CN=$domain

[v3_req]
subjectAltName = $san_entries
EOF

    # Generate certificate signing request and self-sign it
    openssl req -new -key "$key_path" -out /tmp/cert.csr -config "$config_file" 2>/dev/null
    openssl x509 -req -in /tmp/cert.csr -signkey "$key_path" -out "$cert_path" \
        -days 365 -extensions v3_req -extfile "$config_file" 2>/dev/null

    # Clean up
    rm -f /tmp/cert.csr "$config_file"

    # Set proper permissions
    chmod 644 "$cert_path"
    chmod 600 "$key_path"

    log_info "Certificate saved to: $cert_path"
    log_info "Private key saved to: $key_path"
    log_warn "This is a self-signed certificate - NOT suitable for production use"
}

setup_letsencrypt() {
    local domain="$1"
    local email="${LETSENCRYPT_EMAIL:-}"

    # Wildcard certs not supported via HTTP-01 challenge
    if [[ "$domain" == \*.* ]]; then
        log_error "Let's Encrypt wildcard certificates require DNS-01 challenge"
        log_error "Use certbot with DNS plugin for your DNS provider"
        log_error "Example: certbot certonly --dns-cloudflare -d '$domain'"
        exit 1
    fi

    if [[ -z "$email" ]]; then
        log_error "Email required for Let's Encrypt (use --email or LETSENCRYPT_EMAIL)"
        exit 1
    fi

    # Check if certbot is installed
    if ! command -v certbot &> /dev/null; then
        log_error "certbot not found. Installing..."
        if command -v apt-get &> /dev/null; then
            sudo apt-get update
            sudo apt-get install -y certbot
        elif command -v yum &> /dev/null; then
            sudo yum install -y certbot
        else
            log_error "Could not install certbot. Please install it manually."
            exit 1
        fi
    fi

    log_info "Setting up Let's Encrypt certificate for: $domain"
    log_info "Email: $email"

    # Run certbot
    sudo certbot certonly --standalone \
        --non-interactive \
        --agree-tos \
        --email "$email" \
        -d "$domain"

    # Create symlinks to Let's Encrypt certificates
    local le_cert_path="/etc/letsencrypt/live/$domain/fullchain.pem"
    local le_key_path="/etc/letsencrypt/live/$domain/privkey.pem"

    if [[ -f "$le_cert_path" && -f "$le_key_path" ]]; then
        # If custom paths specified, create symlinks
        if [[ "$SSH_CERT_PATH" != "$le_cert_path" ]]; then
            mkdir -p "$(dirname "$SSH_CERT_PATH")"
            ln -sf "$le_cert_path" "$SSH_CERT_PATH"
            log_info "Created symlink: $SSH_CERT_PATH -> $le_cert_path"
        fi

        if [[ "$SSH_KEY_PATH" != "$le_key_path" ]]; then
            mkdir -p "$(dirname "$SSH_KEY_PATH")"
            ln -sf "$le_key_path" "$SSH_KEY_PATH"
            log_info "Created symlink: $SSH_KEY_PATH -> $le_key_path"
        fi

        log_info "Let's Encrypt certificate installed successfully"
        log_info "Certificate: $le_cert_path"
        log_info "Private key: $le_key_path"

        # Set up auto-renewal
        log_info "Setting up auto-renewal..."
        sudo certbot renew --dry-run

        log_info "Auto-renewal is configured via systemd timer or cron"
    else
        log_error "Certificate generation failed"
        exit 1
    fi
}

verify_certificate() {
    local cert_path="$1"
    local domain="$2"

    if [[ ! -f "$cert_path" ]]; then
        log_error "Certificate file not found: $cert_path"
        return 1
    fi

    log_info "Verifying certificate..."

    # Check certificate details
    openssl x509 -in "$cert_path" -text -noout | grep -A 1 "Subject:"
    openssl x509 -in "$cert_path" -text -noout | grep -A 1 "Subject Alternative Name:" || true

    # Check validity dates
    local not_before=$(openssl x509 -in "$cert_path" -noout -startdate | cut -d= -f2)
    local not_after=$(openssl x509 -in "$cert_path" -noout -enddate | cut -d= -f2)

    log_info "Valid from: $not_before"
    log_info "Valid until: $not_after"

    return 0
}

main() {
    log_info "Certificate Setup - Mode: $MODE"

    # Check if certificates already exist
    if [[ -f "$SSH_CERT_PATH" && -f "$SSH_KEY_PATH" ]]; then
        log_warn "Certificates already exist:"
        log_warn "  Certificate: $SSH_CERT_PATH"
        log_warn "  Key: $SSH_KEY_PATH"
        if [[ "$overwrite" == "unset" ]]; then
            read -p "Overwrite? (y/N): " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                log_info "Keeping existing certificates"
                exit 0
            fi
        elif [[ "$overwrite" == "true" ]]; then
            log_info "Overwriting existing certificates (overwrite=true)"
        else
            log_info "Keeping existing certificates (overwrite=false)"
            exit 0
        fi
    fi

    case "$MODE" in
        dev)
            generate_self_signed_cert "$DOMAIN" "$SSH_CERT_PATH" "$SSH_KEY_PATH"
            verify_certificate "$SSH_CERT_PATH" "$DOMAIN"
            ;;
        prod)
            setup_letsencrypt "$DOMAIN"
            ;;
    esac

    log_info "Certificate setup complete!"
    echo ""
    log_info "To use these certificates with the proxy:"
    echo "  export SSH_CERT_PATH=$SSH_CERT_PATH"
    echo "  export SSH_KEY_PATH=$SSH_KEY_PATH"
    echo "  ./target/release/proxy"
}

main "$@"
