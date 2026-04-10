#!/usr/bin/env bash
set -euo pipefail

# ACME Client End-to-End Test Script
# ===================================
# This script helps you test the full ACME certificate workflow with Let's Encrypt staging.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "\n${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║  ACME Client End-to-End Test                                 ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}\n"
}

print_section() {
    echo -e "\n${GREEN}▶ $1${NC}"
}

print_info() {
    echo -e "${BLUE}  ℹ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}  ⚠ $1${NC}"
}

print_error() {
    echo -e "${RED}  ✗ $1${NC}"
}

print_success() {
    echo -e "${GREEN}  ✓ $1${NC}"
}

print_header

# ============================================================================
# Step 1: Check Prerequisites
# ============================================================================
print_section "Step 1: Checking Prerequisites"

# Check if nix is available
if ! command -v nix &> /dev/null; then
    print_error "Nix is not installed or not in PATH"
    echo ""
    echo "Please install Nix from: https://nixos.org/download.html"
    exit 1
fi
print_success "Nix is installed"

# Check if flake.nix exists
if [ ! -f "$REPO_ROOT/flake.nix" ]; then
    print_error "Could not find flake.nix at $REPO_ROOT/flake.nix"
    exit 1
fi
print_success "Found flake.nix"

# Check if we're in a nix develop shell (optional)
if [ -n "${IN_NIX_SHELL:-}" ]; then
    print_info "Already in nix develop shell"
else
    print_info "Not in nix develop shell (will enter it when running tests)"
fi

# ============================================================================
# Step 2: Explain Requirements
# ============================================================================
print_section "Step 2: Requirements for E2E Testing"

echo ""
echo "To run the full end-to-end ACME test, you need:"
echo ""
echo -e "  1. ${GREEN}A Domain Name${NC}"
echo "     - You must own a domain (e.g., example.com)"
echo "     - Or a subdomain (e.g., test.example.com)"
echo ""
echo -e "  2. ${GREEN}DNS Configuration${NC}"
echo "     - The domain must have an A record pointing to this server"
echo "     - Verify: dig <your-domain> (should show this server's IP)"
echo ""
echo -e "  3. ${GREEN}Port 80 Access${NC}"
echo "     - The test needs to bind to port 80 (requires sudo/root)"
echo "     - Port 80 must be accessible from the internet"
echo "     - Verify: nc -l 80 (from another machine: curl http://your-domain)"
echo ""
echo -e "  4. ${GREEN}Firewall Rules${NC}"
echo "     - Ensure port 80 is open in your firewall"
echo "     - Check iptables, cloud provider security groups, etc."
echo ""
echo -e "  5. ${GREEN}Let's Encrypt Staging${NC}"
echo "     - This test uses staging environment only (safe for testing)"
echo "     - Staging certificates are NOT trusted by browsers"
echo "     - No rate limits, safe to experiment"
echo ""

# ============================================================================
# Step 3: Gather Configuration
# ============================================================================
print_section "Step 3: Configuration"

echo ""

# Get email
if [ -n "${ACME_TEST_EMAIL:-}" ]; then
    print_info "Using ACME_TEST_EMAIL from environment: $ACME_TEST_EMAIL"
    TEST_EMAIL="$ACME_TEST_EMAIL"
else
    TEST_EMAIL="admin@vers.sh"
fi

# Get domain
if [ -n "${ACME_TEST_DOMAIN:-}" ]; then
    print_info "Using ACME_TEST_DOMAIN from environment: $ACME_TEST_DOMAIN"
    TEST_DOMAIN="$ACME_TEST_DOMAIN"
else
    read -p "Domain name: " TEST_DOMAIN
    if [ -z "$TEST_DOMAIN" ]; then
        print_error "Domain is required"
        exit 1
    fi
fi

# Get port (default 80)
if [ -n "${ACME_TEST_HTTP_PORT:-}" ]; then
    print_info "Using ACME_TEST_HTTP_PORT from environment: $ACME_TEST_HTTP_PORT"
    print_warn "ACME servers might not work with any other port than 80"
    TEST_PORT="$ACME_TEST_HTTP_PORT"
else
    print_info "Using default port 80"
    TEST_PORT="80"
fi

# ============================================================================
# Step 4: Verify DNS
# ============================================================================
print_section "Step 4: Verifying DNS Configuration"

echo ""
print_info "Checking DNS for $TEST_DOMAIN..."

if command -v dig &> /dev/null; then
    DNS_RESULT=$(dig +short "$TEST_DOMAIN" A | head -n1)
    if [ -n "$DNS_RESULT" ]; then
        print_success "DNS resolves to: $DNS_RESULT"

        # Try to get this server's public IP
        if command -v curl &> /dev/null; then
            SERVER_IP=$(curl -s ifconfig.me || echo "")
            if [ -n "$SERVER_IP" ]; then
                print_info "This server's public IP: $SERVER_IP"
                if [ "$DNS_RESULT" = "$SERVER_IP" ]; then
                    print_success "DNS correctly points to this server!"
                else
                    print_warning "DNS IP ($DNS_RESULT) does NOT match this server's IP ($SERVER_IP)"
                    print_warning "The test may fail if Let's Encrypt cannot reach this server"
                fi
            fi
        fi
    else
        print_error "Could not resolve DNS for $TEST_DOMAIN"
        print_warning "Make sure your domain's A record points to this server"
    fi
else
    print_warning "dig command not found, skipping DNS verification"
fi

# ============================================================================
# Step 5: Check Port Availability
# ============================================================================
print_section "Step 5: Checking Port Availability"

echo ""
if [ "$TEST_PORT" = "80" ]; then
    print_warning "Port 80 requires root/sudo privileges"

    # Check if something is already listening on port 80
    if command -v ss &> /dev/null; then
        if ss -ln | grep -q ":80 "; then
            print_warning "Something is already listening on port 80"
            echo ""
            ss -lnp | grep ":80 " || true
            echo ""
            print_warning "You may need to stop the service using port 80"
        else
            print_success "Port 80 is available"
        fi
    fi
fi

# ============================================================================
# Step 6: Configuration Summary
# ============================================================================
print_section "Step 6: Configuration Summary"

echo ""
echo -e "  Email:     ${GREEN}$TEST_EMAIL${NC}"
echo -e "  Domain:    ${GREEN}$TEST_DOMAIN${NC}"
echo -e "  Port:      ${GREEN}$TEST_PORT${NC}"
echo ""
print_info "Note: Wildcard domains (*.domain.com) require DNS-01, not HTTP-01"
echo ""

# Change to the crate directory
cd "$SCRIPT_DIR"

# Export environment variables
export ACME_TEST_EMAIL="$TEST_EMAIL"
export ACME_TEST_DOMAIN="$TEST_DOMAIN"
export ACME_TEST_HTTP_PORT="$TEST_PORT"

# Note: ACME_TEST_DOMAIN_2 is not used in HTTP-01 test
# Wildcard domains (*.example.com) require DNS-01 challenges, not HTTP-01

# Get the full path to nix
NIX_PATH=$(which nix)
if [ -z "$NIX_PATH" ]; then
    print_error "Could not find nix in PATH"
    exit 1
fi

# ============================================================================
# Step 7: Run Full E2E Test
# ============================================================================
print_section "Step 7: Running Full E2E Test"

echo ""
print_info "Starting test: cargo test test_full_e2e_with_http_server -- --ignored --nocapture"
echo "    1. Start an HTTP server on port $TEST_PORT"
echo "    2. Use pre-configured test ACME account"
echo "    3. Request a certificate for $TEST_DOMAIN"
echo "    4. Automatically serve HTTP-01 challenges"
echo "    5. Wait for Let's Encrypt to validate (may take up to 5 minutes)"
echo "    6. Retrieve and display the certificate"

echo ""

print_info "Using sudo for port 80 access..."

# Check if we have sudo access
if ! sudo -v 2>/dev/null; then
    print_error "Sudo access required for port 80"
    exit 1
fi

# Configure Git to trust the repository when running with sudo
print_info "Configuring Git to trust repository..."
sudo git config --global --add safe.directory "$REPO_ROOT" 2>/dev/null || true

# Run in nix develop with sudo, preserving PATH
sudo -E env "PATH=$PATH" "$NIX_PATH" develop "$REPO_ROOT#default" --command bash -c "
    cd '$SCRIPT_DIR'
    cargo test --test integration_test test_full_e2e_with_http_server -- --ignored --nocapture
"

TEST_EXIT_CODE=$?

echo ""
if [ $TEST_EXIT_CODE -eq 0 ]; then
    print_success "Full E2E test completed successfully!"
    echo ""
    echo "  ✓ Certificate obtained for $TEST_DOMAIN"
    echo ""
    echo "  Your staging certificate was obtained successfully."
    echo "  This certificate is NOT trusted by browsers (staging only)."
    echo ""
else
    print_error "Full E2E test failed with exit code $TEST_EXIT_CODE"
    echo ""
    echo "  Common issues:"
    echo "    - DNS not pointing to this server"
    echo "    - Port 80 not accessible from internet"
    echo "    - Firewall blocking incoming connections"
    echo "    - Another service using port 80"
    echo ""
    echo "  Check the output above for specific error messages."
fi

exit $TEST_EXIT_CODE
