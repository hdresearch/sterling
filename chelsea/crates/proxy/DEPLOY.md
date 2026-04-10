# Proxy Deployment Guide

## Overview

The proxy service is deployed using Docker Compose.

## Architecture

- **Port 80**: Main HTTP proxy service (receives traffic from AWS ELB)
- **Port 443**: Direct TLS connections (currently mapped but not used)
- **Port 51822**: WireGuard VPN (TCP and UDP)
- **Port 9090**: Admin interface (internal only, health checks, metrics)

## Secrets Management

All sensitive configuration is stored in AWS Secrets Manager:
- `PROXY_PRIVATE_KEY` - WireGuard private key
- `ORCHESTRATOR_PUBLIC_KEY` - Orchestrator's WireGuard public key
- `DATABASE_URL` - PostgreSQL connection string
- `ORCHESTRATOR_PUBLIC_IP` - Orchestrator's public IPv4 address
- `ORCHESTRATOR_PRIVATE_IP` - Orchestrator's private IPv6 address (WireGuard)
- `ORCHESTRATOR_PORT` - Orchestrator's HTTP port
- `ORCHESTRATOR_HOST` - Hostname that should route to the Orchestrator API (default `api.vers.sh`)


Ensure TLS certificates are stored:

```bash
# Store certificate
aws secretsmanager create-secret \
  --name proxy/tls-cert \
  --description "Proxy TLS certificate" \
  --secret-string "$(cat /path/to/cert.pem)" \
  --region us-east-1

# Store private key
aws secretsmanager create-secret \
  --name proxy/tls-key \
  --description "Proxy TLS private key" \
  --secret-string "$(cat /path/to/key.pem)" \
  --region us-east-1
```

## Automated Deployment (CI/CD)

The GitHub Actions workflow automatically:
1. Builds and tests the proxy
2. Builds a Docker image
3. Pushes to ECR
4. Deploys using the `deploy.sh` script

The workflow runs on pushes to `production`

### Workflow File

`.github/workflows/proxy.yaml`

The workflow:
1. Fetches secrets from AWS Secrets Manager (`proxy/config`)
2. Fetches TLS certificates from AWS Secrets Manager (`proxy/tls-cert`, `proxy/tls-key`)
3. Builds the Docker image with certificates baked in
4. Deploys with environment variables

## TLS Certificates

TLS certificates are baked into the Docker image during the CI/CD build process:
- The GitHub Actions workflow fetches certificates from AWS Secrets Manager
- Certificates are copied into the image at build time:
  - `/etc/ssl/chelsea/proxy-cert.pem` (certificate)
  - `/etc/ssl/chelsea/proxy-key.pem` (private key)
- Secrets: `proxy/tls-cert` and `proxy/tls-key` in AWS Secrets Manager

**Note**: If certificates need to be rotated, a new Docker image must be built and deployed.

## Viewing Logs

```bash
# Follow logs in real-time
docker-compose logs -f

# View last 100 lines
docker-compose logs --tail=100

# View logs for specific time period
docker-compose logs --since 10m

# View logs with timestamps
docker-compose logs -t
```

Logs are also sent to Loki at `http://172.31.64.14:3100` with labels:
- `service=proxy`
- `container_name=proxy`
- `image_tag=<TAG>`

## Health Checks

The proxy exposes a `/health` endpoint on port 80:

```bash
# Local health check
curl http://localhost/health

# External health check (from ELB)
curl http://<proxy-ip>/health
```

The AWS Load Balancer performs health checks on this endpoint.

## Monitoring

### Admin Endpoints

The admin interface (port 9090, localhost only) provides:

- `/admin/metrics` - Detailed metrics (requires API key)
- `/admin/wireguard/peers` - WireGuard peer information (requires API key)

### Metrics

The proxy logs metrics every 60 seconds:
- SSH connections (total and active)
- HTTP connections (total and active)

## Troubleshooting

### Container keeps restarting

Check logs for errors:
```bash
docker-compose logs --tail=50
```

Common issues:
- **Missing environment variables**: Ensure all required vars are set
- **Invalid WireGuard keys**: `InvalidLength(0)` error means a key is empty
- **Database connection failure**: Check `DATABASE_URL` and network connectivity
- **WireGuard initialization failure**: Container needs `privileged: true` mode

### Check container status

```bash
docker-compose ps
docker inspect proxy
```

### Verify environment variables

```bash
docker exec proxy env | grep -E "(PROXY|ORCHESTRATOR|DATABASE)"
```

### Test database connection

```bash
docker exec proxy psql "$DATABASE_URL" -c "SELECT 1"
```

### Check WireGuard interface

```bash
docker exec proxy wg show
```

### Manual container start (debugging)

```bash
# Stop docker-compose
docker-compose down

# Run interactively
docker run --rm -it --privileged \
  -e PROXY_PRIVATE_KEY="..." \
  -e ORCHESTRATOR_PUBLIC_KEY="..." \
  -e DATABASE_URL="..." \
  -e ORCHESTRATOR_PUBLIC_IP="..." \
  -e ORCHESTRATOR_PRIVATE_IP="..." \
  -e ORCHESTRATOR_PORT="8090" \
  -e PROXY_PORT="80" \
  -e SSH_PORT="80" \
  -e RUST_LOG="debug" \
  -p 80:80 -p 443:443 -p 51822:51822/udp \
  993161092587.dkr.ecr.us-east-1.amazonaws.com/proxy:TAG
```

### 502 Bad Gateway from ELB

This usually means:
- Proxy container is not running
- Health checks are failing
- Proxy is not listening on port 80

Check:
1. Container status: `docker-compose ps`
2. Port bindings: `docker port proxy`
3. Health endpoint: `curl http://localhost/health`
4. ELB target health: `aws elbv2 describe-target-health --target-group-arn <arn>`

## Stopping the Service

```bash
docker-compose down
```

## Updating Configuration

To update secrets without redeploying:

```bash
# Update the secret in AWS
aws secretsmanager update-secret \
  --secret-id proxy/config \
  --secret-string '{"PROXY_PRIVATE_KEY":"...","ORCHESTRATOR_PUBLIC_KEY":"...",...}' \
  --region us-east-1

# Restart the container to pick up new values
docker-compose restart
```

## Security Notes

- ✅ Secrets are stored in AWS Secrets Manager, not in git
- ✅ `.env` file is in `.gitignore`
- ✅ Admin interface only accessible from localhost
- ✅ API key required for admin endpoints
- ✅ TLS certificates fetched from Secrets Manager
- ✅ Database passwords are never logged

## Certificate Rotation

To rotate TLS certificates:

1. Update the certificates in AWS Secrets Manager:
   ```bash
   aws secretsmanager update-secret \
     --secret-id proxy/tls-cert \
     --secret-string "$(cat /path/to/new-cert.pem)" \
     --region us-east-1

   aws secretsmanager update-secret \
     --secret-id proxy/tls-key \
     --secret-string "$(cat /path/to/new-key.pem)" \
     --region us-east-1
   ```

2. Trigger a new build by pushing to the branch:
   ```bash
   git commit --allow-empty -m "Trigger rebuild for cert rotation"
   git push
   ```

3. The CI/CD pipeline will build a new image with the updated certificates and deploy automatically.

## AWS Resources

- **ECR Repository**: `993161092587.dkr.ecr.us-east-1.amazonaws.com/proxy`
- **Secrets Manager**: `proxy/config`, `proxy/tls-cert`, `proxy/tls-key`
- **Load Balancer**: `temporary-proxy-alb`
- **Target Group**: `production-proxy`
- **RDS Database**: `vers.cwxoqiosmfyv.us-east-1.rds.amazonaws.com`

## Support

For issues or questions:
1. Check the logs first
2. Verify all environment variables are set correctly
3. Ensure AWS credentials have access to Secrets Manager
4. Check the GitHub Actions workflow for CI/CD issues
