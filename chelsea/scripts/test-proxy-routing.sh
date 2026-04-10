#!/bin/bash
#
# Test script for proxy routing fixes:
# 1. URI fix - VM receives relative path (/path), not absolute URI (http://host/path)
# 2. Port routing - proxy:X routes to VM:X (no port translation)
# 3. /health routing - VM /health is not intercepted by proxy
# 4. SSH-over-TLS still works
#
# Prerequisites:
# - Single-node environment running (./scripts/single-node.sh start -d)
# - curl, jq, openssl installed
#
# Usage:
#   ./scripts/test-proxy-routing.sh
#

set -e

# Configuration
PROXY_HOST="${PROXY_HOST:-127.0.0.1}"
PROXY_PORT="${PROXY_PORT:-8080}"
ORCH_HOST="${ORCH_HOST:-api.vers.sh}"
VM_PORT="${VM_PORT:-8080}"  # Port the test server will listen on

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

passed=0
failed=0

log_info() {
    echo -e "${YELLOW}[INFO]${NC} $1"
}

log_pass() {
    echo -e "${GREEN}[PASS]${NC} $1"
    ((++passed))
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    ((++failed))
}

cleanup() {
    log_info "Cleaning up..."
    if [[ -n "$VM_ID" ]]; then
        log_info "Deleting VM $VM_ID"
        ./public-api.sh delete "$VM_ID" --skip-wait-boot >/dev/null 2>&1 || true
    fi
    if [[ -n "$SSH_KEY_FILE" && -f "$SSH_KEY_FILE" ]]; then
        rm -f "$SSH_KEY_FILE"
    fi
}

trap cleanup EXIT

# Check prerequisites
log_info "Checking prerequisites..."

if ! command -v curl &> /dev/null; then
    log_fail "curl is required but not installed"
    exit 1
fi

if ! command -v jq &> /dev/null; then
    log_fail "jq is required but not installed"
    exit 1
fi

if ! command -v openssl &> /dev/null; then
    log_fail "openssl is required but not installed"
    exit 1
fi

# Check if proxy is running
if ! curl -sk "https://${PROXY_HOST}:${PROXY_PORT}/health" -H "Host: ${ORCH_HOST}" >/dev/null 2>&1; then
    log_fail "Proxy not responding. Is single-node running?"
    exit 1
fi

log_pass "Prerequisites check"

# Create a VM
log_info "Creating test VM..."
VM_RESPONSE=$(./public-api.sh new --vcpu 2 --mem 1024 --fs 2048 --wait-boot 2>&1)
VM_ID=$(echo "$VM_RESPONSE" | jq -r '.vm_id')

if [[ -z "$VM_ID" || "$VM_ID" == "null" ]]; then
    log_fail "Failed to create VM: $VM_RESPONSE"
    exit 1
fi

log_pass "Created VM: $VM_ID"
VM_HOST="${VM_ID}.vm.vers.sh"

# Get SSH key
log_info "Getting SSH key..."
SSH_KEY_FILE=$(mktemp)
SSH_KEY_RESPONSE=$(./public-api.sh ssh-key "$VM_ID" 2>&1)
echo "$SSH_KEY_RESPONSE" | jq -r '.ssh_private_key' > "$SSH_KEY_FILE"
chmod 600 "$SSH_KEY_FILE"

if [[ ! -s "$SSH_KEY_FILE" ]] || grep -q "null" "$SSH_KEY_FILE"; then
    log_fail "Failed to get SSH key"
    exit 1
fi

log_pass "Got SSH key"

# SSH command helper
ssh_cmd() {
    ssh -i "$SSH_KEY_FILE" \
        -o StrictHostKeyChecking=no \
        -o UserKnownHostsFile=/dev/null \
        -o ConnectTimeout=10 \
        -o LogLevel=ERROR \
        -o ProxyCommand="openssl s_client -connect ${PROXY_HOST}:${PROXY_PORT} -servername ${VM_HOST} -quiet 2>/dev/null" \
        "root@${VM_HOST}" "$@"
}

# Start test server on VM
log_info "Starting test server on VM (port $VM_PORT)..."
ssh_cmd "nohup python3 -c '
import socket
import json
from http.server import HTTPServer, BaseHTTPRequestHandler

class HTTPServerV6(HTTPServer):
    address_family = socket.AF_INET6

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        response = {
            \"status\": \"ok\",
            \"path_received\": self.path,
            \"host_header\": self.headers.get(\"Host\", \"\")
        }
        self.send_response(200)
        self.send_header(\"Content-Type\", \"application/json\")
        self.end_headers()
        self.wfile.write(json.dumps(response).encode())
    def log_message(self, format, *args):
        pass

print(\"Test server started on port $VM_PORT\", flush=True)
HTTPServerV6((\"::\", $VM_PORT), Handler).serve_forever()
' > /tmp/server.log 2>&1 &
sleep 2
cat /tmp/server.log" 2>/dev/null

log_pass "Test server started"

echo ""
echo "========================================"
echo "         Running Tests"
echo "========================================"
echo ""

# Test 1: URI Fix - VM should receive relative path
log_info "Test 1: URI Fix (relative path)"
RESPONSE=$(curl -sk -H "Host: ${VM_HOST}" "https://${PROXY_HOST}:${PROXY_PORT}/api/test/path?query=value" 2>&1)
PATH_RECEIVED=$(echo "$RESPONSE" | jq -r '.path_received' 2>/dev/null)

if [[ "$PATH_RECEIVED" == "/api/test/path?query=value" ]]; then
    log_pass "VM received relative path: $PATH_RECEIVED"
else
    log_fail "VM received unexpected path: $PATH_RECEIVED (expected /api/test/path?query=value)"
    echo "  Response: $RESPONSE"
fi

# Test 2: /health routing - VM /health should NOT be intercepted
log_info "Test 2: VM /health routing (not intercepted)"
RESPONSE=$(curl -sk -H "Host: ${VM_HOST}" "https://${PROXY_HOST}:${PROXY_PORT}/health" 2>&1)
PATH_RECEIVED=$(echo "$RESPONSE" | jq -r '.path_received' 2>/dev/null)

if [[ "$PATH_RECEIVED" == "/health" ]]; then
    log_pass "VM /health not intercepted, received path: $PATH_RECEIVED"
else
    log_fail "VM /health was intercepted or failed"
    echo "  Response: $RESPONSE"
fi

# Test 3: Proxy /health - orchestrator host should return proxy metrics
log_info "Test 3: Proxy /health (orchestrator host)"
RESPONSE=$(curl -sk -H "Host: ${ORCH_HOST}" "https://${PROXY_HOST}:${PROXY_PORT}/health" 2>&1)
SSH_TOTAL=$(echo "$RESPONSE" | jq -r '.ssh.connections_total' 2>/dev/null)

if [[ "$SSH_TOTAL" =~ ^[0-9]+$ ]]; then
    log_pass "Proxy /health returns metrics (ssh.connections_total: $SSH_TOTAL)"
else
    log_fail "Proxy /health did not return expected metrics"
    echo "  Response: $RESPONSE"
fi

# Test 4: Port routing - request on proxy:8080 should go to VM:8080
log_info "Test 4: Port routing (proxy:${PROXY_PORT} -> VM:${VM_PORT})"
# This is implicitly tested by the above tests since we're using port 8080
# But let's verify the server is actually on the expected port
RESPONSE=$(curl -sk -H "Host: ${VM_HOST}" "https://${PROXY_HOST}:${PROXY_PORT}/port-test" 2>&1)
STATUS=$(echo "$RESPONSE" | jq -r '.status' 2>/dev/null)

if [[ "$STATUS" == "ok" ]]; then
    log_pass "Port routing works (proxy:${PROXY_PORT} -> VM:${VM_PORT})"
else
    log_fail "Port routing failed"
    echo "  Response: $RESPONSE"
fi

# Test 5: SSH-over-TLS
log_info "Test 5: SSH-over-TLS"
SSH_RESULT=$(ssh_cmd "echo 'SSH_OK'" 2>&1)

if [[ "$SSH_RESULT" == *"SSH_OK"* ]]; then
    log_pass "SSH-over-TLS works"
else
    log_fail "SSH-over-TLS failed"
    echo "  Result: $SSH_RESULT"
fi

# Test 6: Host header with port
log_info "Test 6: Host header with port"
RESPONSE=$(curl -sk -H "Host: ${ORCH_HOST}:${PROXY_PORT}" "https://${PROXY_HOST}:${PROXY_PORT}/health" 2>&1)
SSH_TOTAL=$(echo "$RESPONSE" | jq -r '.ssh.connections_total' 2>/dev/null)

if [[ "$SSH_TOTAL" =~ ^[0-9]+$ ]]; then
    log_pass "Host header with port works"
else
    log_fail "Host header with port failed"
    echo "  Response: $RESPONSE"
fi

# Test 7: Case insensitive host matching
log_info "Test 7: Case insensitive host"
RESPONSE=$(curl -sk -H "Host: API.VERS.SH" "https://${PROXY_HOST}:${PROXY_PORT}/health" 2>&1)
SSH_TOTAL=$(echo "$RESPONSE" | jq -r '.ssh.connections_total' 2>/dev/null)

if [[ "$SSH_TOTAL" =~ ^[0-9]+$ ]]; then
    log_pass "Case insensitive host matching works"
else
    log_fail "Case insensitive host matching failed"
    echo "  Response: $RESPONSE"
fi

# Test 8: Admin endpoint on VM host (should route to VM, not block)
log_info "Test 8: Admin endpoint with VM host (not blocked)"
RESPONSE=$(curl -sk -o /dev/null -w "%{http_code}" -H "Host: ${VM_HOST}" "https://${PROXY_HOST}:${PROXY_PORT}/admin/metrics" 2>&1)

if [[ "$RESPONSE" == "200" ]]; then
    log_pass "Admin endpoint with VM host routes to VM (200)"
else
    log_fail "Admin endpoint with VM host returned unexpected status: $RESPONSE"
fi

echo ""
echo "========================================"
echo "         Test Summary"
echo "========================================"
echo ""
echo -e "Passed: ${GREEN}${passed}${NC}"
echo -e "Failed: ${RED}${failed}${NC}"
echo ""

if [[ $failed -gt 0 ]]; then
    exit 1
fi

exit 0
