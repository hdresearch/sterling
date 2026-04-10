# Proxy

Vers proxy - A multiplexed reverse proxy that handles both HTTP/HTTPS traffic and SSH-over-TLS connections to VMs.

## Deployment

**For deployment instructions, see [DEPLOY.md](./DEPLOY.md)**

The proxy uses Docker Compose and fetches secrets from AWS Secrets Manager. All sensitive configuration (WireGuard keys, database credentials, etc.) is never stored in source control.

Quick links:
- [Automated CI/CD Deployment](./DEPLOY.md#automated-deployment-cicd)
- [Manual Deployment](./DEPLOY.md#manual-deployment)
- [Secrets Management](./DEPLOY.md#secrets-management)
- [Troubleshooting](./DEPLOY.md#troubleshooting)

## What this does

The proxy is a stand alone, level 7 (HTTP/HTTPS) reverse proxy for both our
internal APIs and customer traffic.

It's responsibilities are:

1. A filter – traffic not bound for our (api.vers.sh) APIs, a VM, a cluster or a
   known client domain is dropped.

2. Rate limiting of requests bound for our APIs. Rate limiting is by IP address.

3. Terminating SSL connections. Including provisioning ACME certs for client
   domains. HTTP connections are automatically redirected to HTTPS connections.

4. Forward traffic bound for our APIs to the orchestrator. Forward traffic bound
   for customer VMs to the respective VM.

5. Manage wireguard endpoints.


## Planned functionality

1. Additionally/Optionally be able to proxy traffic at level 4 (transport layer,
   TCP/UDP).

2. Share rate limiting information across proxy nodes.

3. Traffic bound for our internal APIs passes over a control plane network
   (secured with wireguard) and setup by the proxy.

4. Customer's traffic passes over a per-customer virtual private network also
   secured with wireguard and managed by the proxy.

## Cert issuance

See [Certificate Management](#Certificate Management)

## Networking

- Proxy is center of gravity for networking.
- Its internal IP address is constant {{what should it be?}} TODO
- There needs to be an easy way to find its public IP address. `api.vers.sh`
  maybe?
- The proxy's wireguard public key needs to be well known.
- IPv6 address are partitioned like: ??? TODO See discord #engineering
- Wireguard interfaces should set `PersistentKeepalive = 45` or otherwise
  initiated a connection towards the proxy on startup. Then they can move around
  freely as needed to stay with the nodes.
- The orchestrator needs to have a well known internal IP address

Using this DNS schema

```sh
{UUID}.vm.vers.sh
{UUID}.cluster.vers.sh
```

Where the UUID is a short UUID.

Wildcard DNS entries for `*.vm.vers.sh` and `*.cluster.vers.sh` are setup in
Cloudflare.

## Try it out

We depend on having a connection to Postgres. For local development we use the
config in the `pg` folder in this repo. Insure that postgres is up and running
before proceeding.

### For development

Set

```sh
export DEV=true
```

Then we will default to connecting to postgres on
`postgres://postgres:opensesame@localhost:5432/vers` to override, set the `DATABASE_URL`
environment variable to the desired connection string.

We will bind to 127.0.0.1 port 8080

We will create a wireguard interface, using the dev WG keys.

The default location of the orchestrator is http://127.0.0.0:3000, to override
set the env variables `ORCHESTRATOR_PRV_IP` and `ORCHESTRATOR_PORT`. The proxy
also routes `Host: api.vers.sh` to the orchestrator by default; override that
domain with `ORCHESTRATOR_HOST` if you need a different API hostname.

**Note** you don't have to do anything with Wireguard for dev. You _can_ set the
`ORCHESTRATOR_PRV_IP` to one only reachable via wireguard, but you can also
leave it at the default.

A reasonable way to mock orchestrator for dev is to grab a basic web server like
https://github.com/ksdme/ut and run it like so `ut serve --host 127.0.0.1 --port
3000` and observe that the appropriate requests are passed to it.


### For production

And set the following values:

```sh
export DATABASE_URL="pg connection string"
export PROXY_PRV_KEY="desired WG private key for the proxy"
export ORCHESTRATOR_PUB_KEY="Orchestrator's public key"
export ORCHESTRATOR_PUB_IP="The IP we can reach orchestrator at"
export ORCHESTRATOR_PORT=8090
export ORCHESTRATOR_HOST=api.prod.vers.sh
export ORCHESTRATOR_PRV_IP=fd00:fe11:deed::ffff
```

Setting any of these values will override their defaults.

## Example Curl tests

```bash
# Test API endpoint
curl -H "Authorization: Bearer 7d12d800-f467-4c1b-9163-15f5e7102179kAiByMOc1nLKdIqoHD7PrNopJdG3LO3f" \
  -H "Host: api.vers.sh" \
  http://127.0.0.1:8080/

# Check health/metrics
curl http://localhost:8080/health
```

---

## SSH-over-TLS Feature

The proxy supports SSH-over-TLS, allowing users to SSH into VMs through the proxy using TLS+SNI on a single port (typically 443). This eliminates the need for port ranges and works with standard SSH clients.

### Architecture

```
SSH Client
  ↓ TLS connection with SNI (e.g., <vm-id>.vm.vers.sh)
Proxy (Protocol Detection: TLS vs HTTP)
  ↓ TLS termination, extract SNI, lookup VM in database
  ↓ WireGuard encrypted tunnel
Chelsea Node
  ↓ Namespace routing
VM (Firecracker guest with IPv6)
  ↓ SSH Server
```

**Key Features:**
- **Protocol Multiplexing**: HTTP and SSH-over-TLS coexist on the same port
- **SNI-based Routing**: Single wildcard cert (`*.vm.vers.sh`) covers all VMs
- **WireGuard Transport**: VM traffic encrypted over WireGuard IPv6 network
- **Direct VM Peering**: Each VM has its own WireGuard peer connection to proxy

### Configuration

#### Environment Variables

```bash
# HTTP Configuration
DEV=1                              # Enable dev mode (localhost:8080)
DATABASE_URL="postgresql://..."              # Database connection string

# WireGuard Configuration
PROXY_PRV_KEY="..."                # Proxy's WireGuard private key
ORCHESTRATOR_PUB_KEY="..."                 # Orchestrator's WireGuard public key
ORCHESTRATOR_PUB_IP="..."                  # Orchestrator's public IP
ORCHESTRATOR_HOST="api.vers.sh"            # Host header that routes to the orchestrator

# SSH-over-TLS Configuration (always enabled)
SSH_PORT=443                       # SSH-over-TLS port (default: 8443 dev, 443 prod)
SSH_CERT_PATH=/path/to/cert.pem    # TLS certificate path (default: /etc/ssl/chelsea/proxy-cert.pem)

# Timeout Configuration
SSH_TLS_HANDSHAKE_TIMEOUT=10       # TLS handshake timeout seconds (default: 10)
SSH_BACKEND_CONNECT_TIMEOUT=10     # Backend connection timeout seconds (default: 10)
SSH_IDLE_TIMEOUT=0                 # Idle timeout seconds (default: 0/disabled)

# Logging
RUST_LOG=info                      # Log level (debug, info, warn, error)
```

### Client Connection Examples

#### Using OpenSSL ProxyCommand

```bash
# SSH to a VM through the proxy
ssh -o ProxyCommand="openssl s_client -connect proxy.vers.sh:443 -servername <vm-id>.vm.vers.sh -quiet 2>/dev/null" \
  root@<vm-id>.vm.vers.sh
```

#### SSH Config File

Add to `~/.ssh/config`:

```
Host *.vm.vers.sh
    ProxyCommand openssl s_client -connect proxy.vers.sh:443 -servername %h -quiet 2>/dev/null
    StrictHostKeyChecking no
    UserKnownHostsFile ~/.ssh/known_hosts_vers
```

Then simply:
```bash
ssh root@<vm-id>.vm.vers.sh
```

### Certificate Management

Proxy looks in pg to query TLS-certs at the `tls_certs` table. here lies all certs
for custom domains, but also a special case.

The TLS cert for `api.vers.sh`/`*.vm.vers.sh` also lies here. Because these certs
don't belong to any other resource, a special "magic value" has been used is the
primary key. That primary is this:
```text
b0e4346b-302e-49c4-9692-4dbfdf8b2cbc
```

#### Development (Self-Signed)

For testing, insert a self-signed certificate into the db at said special value
primary key:

```bash
./pg/scripts/insert-vers-tls-db.sh
```

> [!NOTE]
> SNE already does this for you.

#### Production (Let's Encrypt)

Currently proxy expects it's own certificate to be located at said magic value
primary key and it doesn't do any effort in renewal of certs.

For customers custom domain certs, proxy auto-generates them on the fly when
they are needed. Proxy does not make an effort currently to renew them.

**Wildcard Certificate for `*.vm.vers.sh`**:

Wildcard certificates require DNS-01 challenge. Using certbot with Route53:

```bash
# Install certbot with Route53 plugin
sudo apt-get install certbot python3-certbot-dns-route53

# Configure AWS credentials with Route53 permissions:
# - route53:ListHostedZones
# - route53:GetChange
# - route53:ChangeResourceRecordSets

# Request wildcard cert (automated)
sudo certbot certonly \
  --dns-route53 \
  -d '*.vm.vers.sh' \
  -d 'api.vers.sh' \
  --agree-tos \
  --email admin@vers.sh \
  --non-interactive

# Certificates saved to:
# /etc/letsencrypt/live/vm.vers.sh/fullchain.pem (certificate)
# /etc/letsencrypt/live/vm.vers.sh/privkey.pem (private key)
```

Put that cert in the `tls_certs` table at primary key:
```text
b0e4346b-302e-49c4-9692-4dbfdf8b2cbc
```

**Using the Certificate**:

```bash
SSH_CERT_PATH=/etc/letsencrypt/live/vm.vers.sh/fullchain.pem \
  ./target/release/proxy
```

**Certificate Renewal**:

Certificates expire every 90 days. Certbot automatically renews them. To reload the proxy after renewal:

```bash
# Option 1: Restart proxy (simple)
systemctl restart proxy

# Option 2: Implement hot reload (future enhancement)
```

### Firewall Requirements

#### Proxy Server

**Inbound**:
- TCP 80 (HTTP)
- TCP 443 (HTTPS + SSH-over-TLS)
- UDP 51820 (WireGuard) from Chelsea nodes

**Outbound**:
- UDP 51820 (WireGuard) to Chelsea nodes
- TCP 5432 (PostgreSQL) to database
- TCP 3000 (or configured) to orchestrator

#### Chelsea Nodes

**Inbound**:
- UDP 51820 (WireGuard) from proxy

**Outbound**:
- UDP 51820 (WireGuard) to proxy

### Monitoring & Health Checks

#### Health Check Endpoint

```bash
curl http://localhost:8080/health
```

Returns JSON metrics:
```json
{
  "ssh": {
    "connections_total": 42,
    "connections_active": 3,
    "errors": {
      "tls_handshake": 0,
      "backend_connection": 1,
      "vm_not_found": 2,
      "other": 0
    }
  },
  "http": {
    "connections_total": 1523,
    "connections_active": 12
  }
}
```

#### Logs

Metrics are automatically logged every 60 seconds:

```
INFO Metrics summary ssh_total=42 ssh_active=3 http_total=1523 http_active=12
```

Control log verbosity with `RUST_LOG`:
```bash
RUST_LOG=debug ./target/release/proxy  # Verbose logging
RUST_LOG=info ./target/release/proxy   # Standard logging
RUST_LOG=error ./target/release/proxy  # Errors only
```

#### WireGuard Status

```bash
# Check WireGuard interface
sudo wg show wgproxy

# Check specific peer
sudo wg show wgproxy | grep -A 5 "<public-key>"
```

### Troubleshooting

#### SSH Connection Fails

**Symptom**: SSH hangs or times out

**Checks**:
1. **Verify WireGuard connectivity**:
   ```bash
   # On proxy
   sudo wg show wgproxy
   # Look for recent handshake with VM peer

   # Ping VM over WireGuard
   ping6 fd00:fe11:deed:1234::<vm-ip>
   ```

2. **Check proxy logs**:
   ```bash
   # If running in background
   tail -f /tmp/proxy.log

   # Look for errors:
   # - "VM not found" → VM not in database
   # - "Backend connection timeout" → Can't reach VM
   # - "TLS handshake failed" → Certificate or TLS issue
   ```

3. **Verify VM is in database**:
   ```bash
   psql $DATABASE_URL -c "SELECT vm_id, ip FROM vms WHERE vm_id = '<vm-id>';"
   ```

4. **Test direct SSH to VM**:
   ```bash
   # Via WireGuard IPv6
   ssh root@fd00:fe11:deed:1234::<vm-ip>
   ```

#### WireGuard Handshake Fails

**Symptom**: `0 B received` in `wg show`, no handshake

**Checks**:
1. **Firewall**: Ensure UDP 51820 is allowed inbound
2. **Security Groups**: AWS/cloud firewall allows UDP 51820
3. **VM peer configuration**: VM has correct proxy public key and endpoint

#### TLS Handshake Timeout

**Symptom**: "TLS handshake timeout" in logs

**Possible Causes**:
- Network connectivity issues
- Certificate problems
- Client not completing handshake

**Fix**:
- Increase timeout: `SSH_TLS_HANDSHAKE_TIMEOUT=20`
- Check client connectivity
- Verify certificate is valid

#### VM Not Found

**Symptom**: "VM not found for hostname" in logs

**Cause**: VM doesn't exist in database or hostname is malformed

**Fix**:
- Verify VM exists: `psql $DATABASE_URL -c "SELECT * FROM vms;"`
- Check hostname format: Must be valid UUID like `abc123-def456-...vm.vers.sh`
- Verify first 36 characters are parseable as UUID

#### Backend Connection Timeout

**Symptom**: "Backend connection timeout" or "Failed to connect to backend"

**Checks**:
1. **VM is running**: Check Firecracker process on Chelsea node
2. **VM has IPv6 configured**: `ip -6 addr` shows WireGuard IPv6
3. **SSH is listening**: `ss -tlnp | grep :22` in VM
4. **WireGuard peer exists**: `sudo wg show wgproxy | grep <vm-public-key>`

### Metrics & Observability

The proxy tracks:
- **SSH Connections**: Total, active, errors by type
- **HTTP Connections**: Total, active
- **WireGuard Peers**: Via `wg show`

Access via:
- **Health endpoint**: `GET /health`
- **Logs**: Periodic metrics every 60 seconds
- **WireGuard**: `sudo wg show wgproxy`

### Testing

```bash
# Run unit tests
cargo test --package proxy

# Run protocol detection tests
cargo test --package proxy protocol::tests

# Run with WireGuard (requires sudo)
sudo -E cargo test --package proxy --test vm_connectivity -- --ignored --nocapture
```

## Testing Custom-domain TLS

### Additional Documentation

- **Manual Setup Guide**: See `SSH-OVER-TLS-MANUAL-SETUP.md`
- **VM Connectivity**: See `VM-CONNECT-STEPS.md`
- **Automation Plan**: See `AUTOMATION-TODO.md`
- **Implementation Plan**: See `SSH_OVER_TLS_PLAN.md`

---
