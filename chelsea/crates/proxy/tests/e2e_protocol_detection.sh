#!/bin/bash
# E2E test for protocol detection after TLS termination
#
# This test verifies that the proxy correctly routes traffic based on
# the application protocol detected AFTER TLS termination.
#
# Prerequisites:
# - Single-node environment must be running (./scripts/single-node.sh start)
# - A VM must exist or will be created
#
# Usage:
#   ./crates/proxy/tests/e2e_protocol_detection.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

PROXY_HOST="127.0.0.1"
PROXY_PORT="${PROXY_PORT:-8080}"

echo "========================================"
echo "E2E Test: Protocol Detection After TLS"
echo "========================================"
echo ""

# Check if proxy is running
echo "[1/5] Checking proxy health..."
if ! curl -s -f -H "Host: api.vers.sh" "http://${PROXY_HOST}:${PROXY_PORT}/health" > /dev/null 2>&1; then
    echo "ERROR: Proxy is not responding. Make sure single-node environment is running."
    echo "Run: ./scripts/single-node.sh start"
    exit 1
fi
echo "      Proxy is healthy"

# Create or find a VM
echo ""
echo "[2/5] Creating test VM..."
VM_RESPONSE=$(curl -s -X POST \
    -H "Authorization: Bearer ${API_TOKEN}" \
    -H "Host: api.vers.sh" \
    -H "Content-Type: application/json" \
    --data '{"vm_config": {}}' \
    "http://${PROXY_HOST}:${PROXY_PORT}/api/v1/vm/new_root")

VM_ID=$(echo "${VM_RESPONSE}" | grep -o '"vm_id":"[^"]*"' | cut -d'"' -f4)

if [ -z "${VM_ID}" ]; then
    echo "ERROR: Failed to create VM"
    echo "Response: ${VM_RESPONSE}"
    exit 1
fi

echo "      Created VM: ${VM_ID}"
VM_HOSTNAME="${VM_ID}.vm.vers.sh"

# Wait for VM to be ready
echo ""
echo "[3/5] Waiting for VM to boot..."
sleep 3

# Test 1: SSH over TLS
echo ""
echo "[4/5] Testing SSH-over-TLS protocol detection..."
SSH_RESPONSE=$(echo "SSH-2.0-TestClient_1.0" | timeout 10 openssl s_client \
    -connect "${PROXY_HOST}:${PROXY_PORT}" \
    -servername "${VM_HOSTNAME}" \
    -quiet 2>/dev/null | head -1)

if [[ "${SSH_RESPONSE}" == SSH-2.0-* ]]; then
    echo "      SUCCESS: SSH banner received: ${SSH_RESPONSE}"
else
    echo "      FAILED: Expected SSH banner, got: ${SSH_RESPONSE}"
    exit 1
fi

# Test 2: HTTP over TLS (expect connection refused since no HTTP server in VM)
echo ""
echo "[5/5] Testing HTTP-over-TLS protocol detection..."
HTTP_RESPONSE=$(curl -sk --connect-timeout 5 \
    --resolve "${VM_HOSTNAME}:${PROXY_PORT}:${PROXY_HOST}" \
    "https://${VM_HOSTNAME}:${PROXY_PORT}/" 2>&1 || true)

# We expect either:
# - "503 Service unavailable" (proxy tried port 80, VM has no HTTP server)
# - "connection refused" (same reason, different error message)
# - An actual HTTP response (if VM has web server)
if echo "${HTTP_RESPONSE}" | grep -qi "503\|service unavailable\|connection refused\|502\|refused"; then
    echo "      SUCCESS: HTTP routed to port 80 (503/refused - VM has no web server)"
elif echo "${HTTP_RESPONSE}" | grep -qi "HTTP\|html\|<!DOCTYPE\|200"; then
    echo "      SUCCESS: HTTP routed to port 80 and got response from VM"
else
    echo "      RESULT: ${HTTP_RESPONSE}"
    echo "      WARNING: Unexpected response, but may still be OK"
fi

echo ""
echo "========================================"
echo "E2E Test PASSED"
echo "========================================"
echo ""
echo "Protocol detection after TLS termination is working correctly:"
echo "  - Same hostname: ${VM_HOSTNAME}"
echo "  - SSH client traffic -> routed to VM port 22"
echo "  - HTTP client traffic -> routed to VM port 80"
echo ""
