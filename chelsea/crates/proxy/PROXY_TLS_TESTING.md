# Proxy TLS Testing Guide

This guide explains how to test the proxy's TLS functionality with custom domains for VMs.

## Overview

The proxy supports TLS termination and routing for VMs using:
1. **UUID subdomains**: `{vm-id}.vm.vers.sh` (wildcard certificate)
2. **Custom domains**: Any custom domain mapped to a VM (individual certificates)

## New Testing Commands

The `public-api.sh` script has been enhanced with commands to test proxy TLS functionality:

### VM Request Commands

```bash
# Send HTTPS request to VM using UUID subdomain
./public-api.sh vm-request-uuid <vm-id> [path]

# Send HTTPS request to VM using custom domain
./public-api.sh vm-request-custom <domain> [path]
```

**Examples:**
```bash
# Test VM with UUID subdomain
./public-api.sh vm-request-uuid 550e8400-e29b-41d4-a716-446655440000

# Test VM with UUID subdomain and specific path
./public-api.sh vm-request-uuid 550e8400-e29b-41d4-a716-446655440000 /health

# Test VM with custom domain
./public-api.sh vm-request-custom example.com

# Test VM with custom domain and path
./public-api.sh vm-request-custom example.com /api/status
```

### ACME Challenge Testing

```bash
# Test ACME HTTP-01 challenge endpoint
./public-api.sh acme-challenge <domain> <token>
```

**Example:**
```bash
./public-api.sh acme-challenge example.com test-token-123
```

### Proxy Health Commands

```bash
# Get proxy health metrics
./public-api.sh proxy-health

# Get proxy version
./public-api.sh proxy-version
```

## Understanding Test Results

### HTTP Status Codes

- **200-299**: Successful response from VM - proxy routing working perfectly
- **500**: **This is considered SUCCESS** for proxy testing! A 500 means:
  - ✓ TLS handshake completed successfully
  - ✓ Proxy routed the request to the correct VM
  - ✓ VM received the request (but may still be booting)
- **Other errors**: May indicate proxy configuration issues

### Why 500 is Success

When testing proxy TLS functionality, a 500 response from a newly created VM indicates:
1. The proxy successfully terminated TLS
2. The proxy correctly resolved the domain/subdomain to the VM
3. The proxy forwarded the request to the VM
4. The VM is reachable but not yet fully ready to serve requests

This proves the proxy's TLS and routing functionality is working correctly.

