# ACME Client End-to-End Testing Guide

This guide explains how to test the `vers_acme` ACME client implementation with Let's Encrypt staging environment.

## Quick Start

The easiest way to run the end-to-end test is using the provided script:

```bash
cd crates/vers_acme
./test-e2e.sh
```

The script will guide you through the entire process interactively.

## Prerequisites

### 1. Domain Name
You need a domain or subdomain that you control. Examples:
- `test.example.com`
- `acme-test.yourdomain.com`
- `example.com`

### 2. DNS Configuration
Your domain must have an **A record** pointing to the server where you're running the test.

**Verify DNS:**
```bash
dig +short your-domain.example.com A
# Should return your server's public IP address
```

### 3. Port 80 Access
The ACME HTTP-01 challenge requires port 80 to be:
- Available (not used by another service)
- Accessible from the internet
- Allowed through firewall rules

**Check if port 80 is in use:**
```bash
sudo ss -lntp | grep :80
```

**Check if port 80 is accessible from outside:**
```bash
# On your local machine (not the server):
curl -v http://your-domain.example.com
```

### 4. Root/Sudo Access
Binding to port 80 requires root privileges. The test script will use `sudo` automatically.

### 5. Nix Development Environment
The project uses Nix for reproducible builds. Make sure you have:
- Nix installed with flakes enabled
- Access to the repository's `flake.nix`

## Running Tests

### Option 1: Interactive Script (Recommended)

```bash
./test-e2e.sh
```

The script will:
1. Check prerequisites (Nix, flake.nix)
2. Explain requirements
3. Prompt for configuration (email, domain, port)
4. Verify DNS configuration
5. Check port availability
6. Run the end-to-end test

### Option 2: Manual Execution

If you prefer to run tests manually:

**Set environment variables:**
```bash
export ACME_TEST_EMAIL="your-email@example.com"
export ACME_TEST_DOMAIN="your-domain.example.com"
export ACME_TEST_HTTP_PORT="80"  # Optional, defaults to 80
```

**Run the test:**
```bash
# Enter nix develop shell
nix develop ../../#default

# Run with sudo for port 80
sudo -E cargo test --test integration_test test_full_e2e_with_http_server -- --ignored --nocapture
```

### Option 3: Using a Different Port

If you can't use port 80 directly, you can use port forwarding:

**Use port 8080 with iptables forwarding:**
```bash
export ACME_TEST_HTTP_PORT="8080"

# Forward port 80 to 8080
sudo iptables -t nat -A PREROUTING -p tcp --dport 80 -j REDIRECT --to-port 8080

# Run test without sudo
nix develop ../../#default
cargo test --test integration_test test_full_e2e_with_http_server -- --ignored --nocapture
```

## Environment Variables

All configuration is done via environment variables:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ACME_TEST_EMAIL` | Yes | - | Email for ACME account registration |
| `ACME_TEST_DOMAIN` | Yes | - | Domain name (must point to this server) |
| `ACME_TEST_DOMAIN_2` | No | `www.your-domain.example.com` | Secondary domain for multi-domain tests |
| `ACME_TEST_HTTP_PORT` | No | `80` | HTTP server port for challenges |
| `ACME_TEST_ACCOUNT_KEY` | No | Empty | Saved account credentials (JSON) for reuse |

## What the Test Does

The `test_full_e2e_with_http_server` test performs a complete ACME workflow:

```
[1/7] Start HTTP server on port 80
      ↓ Binds to 0.0.0.0:80 and waits for challenge requests

[2/7] Create ACME client
      ↓ Registers account with Let's Encrypt staging
      ↓ Saves account credentials for reuse

[3/7] Request certificate
      ↓ Creates order for specified domain(s)
      ↓ Receives HTTP-01 challenges

[4/7] Configure HTTP server
      ↓ Adds challenges to server's response map
      ↓ Server will respond to /.well-known/acme-challenge/{token}

[5/7] Notify ACME server
      ↓ Tells Let's Encrypt that challenges are ready
      ↓ Let's Encrypt will now make HTTP requests to verify

[6/7] Wait for validation
      ↓ Polls order status (may take up to 5 minutes)
      ↓ Let's Encrypt validates domain ownership

[7/7] Finalize and retrieve certificate
      ↓ Generates CSR (Certificate Signing Request)
      ↓ Submits CSR to Let's Encrypt
      ↓ Downloads signed certificate
      ✓ Certificate obtained!
```

## Test Output

Successful test output looks like:

```
╔══════════════════════════════════════════════════════════════╗
║  End-to-End ACME Certificate Test with HTTP Server          ║
╚══════════════════════════════════════════════════════════════╝

Configuration:
  Email:    your-email@example.com
  Domain:   your-domain.example.com
  Port:     80
  Directory: https://acme-staging-v02.api.letsencrypt.org/directory

[1/7] Starting HTTP server on port 80...
  ✓ HTTP server listening on 0.0.0.0:80
  Ready to serve ACME challenges

[2/7] Creating ACME client...
  ✓ ACME client created

  Account credentials (save for reuse):
  {"id":"...","key":"...","directory":"..."}

[3/7] Requesting certificate for domain: your-domain.example.com
  ✓ Certificate order created

[4/7] Configuring HTTP server with challenges...
  Challenge for your-domain.example.com:
    Token: abc123...
    URL:   http://your-domain.example.com/.well-known/acme-challenge/abc123...
    Key Authorization: abc123...xyz789...
  ✓ 1 challenge(s) configured

[5/7] Notifying ACME server that challenges are ready...
  ✓ Challenges marked as ready

[6/7] Waiting for ACME server to validate challenges...
  This may take up to 5 minutes. The ACME server will make HTTP
  requests to http://your-domain.example.com/.well-known/acme-challenge/{token}
  watching for validation requests...

  [HTTP] Request for challenge token: abc123...
  [HTTP] ✓ Serving challenge response
  ✓ All challenges validated successfully!

[7/7] Finalizing order and retrieving certificate...
  ✓ Certificate issued!

╔══════════════════════════════════════════════════════════════╗
║  Certificate Successfully Obtained!                          ║
╚══════════════════════════════════════════════════════════════╝

Certificate Details:
  Domain:         your-domain.example.com
  Expires at:     1234567890 (Unix timestamp)
  Valid for:      89 days
  Cert length:    4567 bytes
  Key length:     1234 bytes

Renewal Status:
  ✓ Valid (> 30 days until expiry)

⚠️  This is a STAGING certificate - not trusted by browsers
   Use for testing only.

✓ Test completed successfully!
```

## Troubleshooting

### DNS Not Resolving

**Problem:** `dig` doesn't return your server's IP

**Solution:**
1. Check your DNS provider's control panel
2. Ensure A record points to correct IP
3. Wait for DNS propagation (can take up to 24 hours)
4. Use `dig @8.8.8.8 your-domain.com` to check Google's DNS

### Port 80 Already In Use

**Problem:** Another service is using port 80

**Solution:**
```bash
# Find what's using port 80
sudo ss -lntp | grep :80

# Stop the service (example for nginx)
sudo systemctl stop nginx

# Or use port forwarding (see Option 3 above)
```

### Port 80 Not Accessible

**Problem:** Let's Encrypt can't reach your server

**Solution:**
1. Check cloud provider security groups (AWS, GCP, Azure)
2. Check firewall rules:
   ```bash
   sudo iptables -L -n | grep 80
   sudo ufw status  # if using ufw
   ```
3. Verify from external network:
   ```bash
   curl -v http://your-domain.com
   ```

### Permission Denied on Port 80

**Problem:** Can't bind to port 80

**Solution:**
- Run with `sudo`: The script does this automatically
- Or use port forwarding to a higher port

### Let's Encrypt Rate Limits

**Problem:** Too many requests

**Solution:**
- This test uses **staging environment** - no rate limits!
- Staging URL: `https://acme-staging-v02.api.letsencrypt.org/directory`
- Safe to run multiple times for testing

### Test Hangs at "Waiting for validation"

**Problem:** Test times out waiting for Let's Encrypt

**Possible causes:**
1. DNS doesn't point to your server
2. Firewall blocking port 80
3. HTTP server not responding correctly
4. Let's Encrypt servers temporarily slow

**Debug:**
```bash
# Check if your HTTP server is responding
curl -v http://your-domain.com/.well-known/acme-challenge/test

# Should return 404 (not found) or 200 with content
# If it times out or refuses connection, fix networking first
```

## Reusing Account Credentials

After the first successful test, you'll see account credentials:

```json
{"id":"https://...","key":"...","directory":"..."}
```

**To reuse the account:**

```bash
export ACME_TEST_ACCOUNT_KEY='{"id":"https://...","key":"...","directory":"..."}'
./test-e2e.sh
```

This avoids creating a new account for each test run.

## Other Tests

### Unit Tests

Run library unit tests (no domain required):

```bash
cargo test --lib
```

### Config Validation Tests

Run non-ignored integration tests:

```bash
cargo test --test integration_test
```

### All Tests

Run everything (only unit tests will actually execute):

```bash
cargo test
```

## Production Usage

⚠️ **Important:** This test uses **Let's Encrypt staging environment only**.

Staging certificates are:
- ✓ Free and unlimited
- ✓ Safe for testing
- ✗ **NOT trusted by browsers**
- ✗ Not suitable for production

To use in production:
1. Change `LETSENCRYPT_STAGING_DIRECTORY` to production URL
2. Implement certificate storage and renewal
3. Use obtained certificates in your application
4. Set up monitoring for certificate expiry

## Additional Resources

- [Let's Encrypt Documentation](https://letsencrypt.org/docs/)
- [ACME Protocol RFC 8555](https://tools.ietf.org/html/rfc8555)
- [Let's Encrypt Staging Environment](https://letsencrypt.org/docs/staging-environment/)
- [instant-acme Documentation](https://docs.rs/instant-acme/)

## Need Help?

If you encounter issues:

1. Read the error messages carefully
2. Check the troubleshooting section above
3. Verify all prerequisites are met
4. Check firewall and DNS configuration
5. Try with a different domain if possible

The test script provides detailed output at each step to help diagnose issues.
