# Sterling Deployment Guide for hdresearch/chelsea Automation

This guide explains how to deploy Sterling for automated SDK generation and documentation updates when OpenAPI specifications change in hdresearch/chelsea.

## Overview

Sterling provides a complete automation pipeline:

```
hdresearch/chelsea (OpenAPI spec changes)
        ↓ GitHub Webhook
Sterling Webhook Server
        ↓ Triggers Pipeline
Sterling SDK Generator
        ↓ Generates & Enhances
├── TypeScript SDK → hdresearch/chelsea-typescript-sdk
├── Rust SDK → hdresearch/chelsea-rust-sdk
├── Python SDK → hdresearch/chelsea-python-sdk
├── Go SDK → hdresearch/chelsea-go-sdk
└── Documentation → hdresearch/vers-docs (Pull Request)
```

## Deployment Options

### Option 1: Cloud Deployment (Recommended)

Deploy Sterling as a webhook server on a cloud platform:

#### Using Railway/Render/Fly.io

1. **Create Dockerfile**:
```dockerfile
FROM alpine:latest

# Install Zig
RUN apk add --no-cache curl tar xz
RUN curl -L https://ziglang.org/download/0.15.2/zig-linux-x86_64-0.15.2.tar.xz | tar -xJ
RUN mv zig-linux-x86_64-0.15.2 /opt/zig
ENV PATH="/opt/zig:$PATH"

# Copy Sterling source
WORKDIR /app
COPY . .

# Build Sterling
RUN zig build -Doptimize=ReleaseFast

# Expose webhook port
EXPOSE 8080

# Start webhook server
CMD ["./zig-out/bin/sterling", "webhook", "--port", "8080"]
```

2. **Set Environment Variables**:
```bash
ANTHROPIC_API_KEY=your_anthropic_key
GITHUB_TOKEN=your_github_token
WEBHOOK_SECRET=your_webhook_secret
NPM_TOKEN=your_npm_token
PYPI_TOKEN=your_pypi_token
```

3. **Deploy**:
```bash
# Railway
railway deploy

# Render
render deploy

# Fly.io
fly deploy
```

#### Using GitHub Actions (Serverless)

Deploy as GitHub Actions workflow triggered by repository dispatch:

```yaml
# .github/workflows/sterling-webhook.yml
name: Sterling Webhook Handler

on:
  repository_dispatch:
    types: [chelsea-openapi-updated]

jobs:
  generate-sdks:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v4
    - uses: goto-bus-stop/setup-zig@v2
      with:
        version: 0.15.2
    - name: Build Sterling
      run: zig build -Doptimize=ReleaseFast
    - name: Generate SDKs
      env:
        ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      run: |
        ./zig-out/bin/sterling generate \
          --spec ${{ github.event.client_payload.openapi_url }} \
          --config sterling.toml \
          --enhance
```

### Option 2: Self-Hosted Deployment

Deploy on your own server:

#### Using Docker Compose

```yaml
# docker-compose.yml
version: '3.8'
services:
  sterling:
    build: .
    ports:
      - "8080:8080"
    environment:
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
      - GITHUB_TOKEN=${GITHUB_TOKEN}
      - WEBHOOK_SECRET=${WEBHOOK_SECRET}
    volumes:
      - ./sterling.toml:/app/sterling.toml
      - ./generated:/app/generated
    restart: unless-stopped
```

#### Using systemd (Linux)

```ini
# /etc/systemd/system/sterling.service
[Unit]
Description=Sterling SDK Generator Webhook Server
After=network.target

[Service]
Type=simple
User=sterling
WorkingDirectory=/opt/sterling
ExecStart=/opt/sterling/zig-out/bin/sterling webhook --port 8080
Restart=always
RestartSec=10

Environment=ANTHROPIC_API_KEY=your_key
Environment=GITHUB_TOKEN=your_token
Environment=WEBHOOK_SECRET=your_secret

[Install]
WantedBy=multi-user.target
```

## Webhook Configuration

### 1. Set Up GitHub Webhook in hdresearch/chelsea

1. Go to https://github.com/hdresearch/chelsea/settings/hooks
2. Click "Add webhook"
3. Configure:
   - **Payload URL**: `https://your-sterling-server.com/webhook/chelsea`
   - **Content type**: `application/json`
   - **Secret**: Your webhook secret
   - **Events**: Select "Push" and "Pull requests"

### 2. Configure Webhook Handler

Sterling's webhook handler is configured in `sterling.toml`:

```toml
[webhook]
enabled = true
port = 8080
path = "/webhook/chelsea"
secret = "${WEBHOOK_SECRET}"
target_repo = "hdresearch/chelsea"
openapi_patterns = [
    "openapi.yaml",
    "openapi.json",
    "api-spec.yaml"
]
```

## Security Considerations

### 1. Webhook Security
- Use a strong webhook secret
- Validate GitHub webhook signatures
- Use HTTPS for webhook endpoints
- Restrict webhook IP ranges if possible

### 2. API Key Management
- Store API keys as environment variables
- Use secret management services (AWS Secrets Manager, etc.)
- Rotate keys regularly
- Use least-privilege access tokens

### 3. Repository Access
- Use fine-grained GitHub tokens
- Limit repository access to only required repos
- Use separate tokens for different operations

## Monitoring and Logging

### 1. Application Monitoring

```bash
# Check Sterling webhook server status
curl -f https://your-sterling-server.com/health

# View logs
docker logs sterling-container
# or
journalctl -u sterling -f
```

### 2. GitHub Actions Monitoring

Monitor workflow runs at:
- https://github.com/your-org/sterling/actions
- Check SDK repository commits
- Monitor vers-docs pull requests

### 3. Alerting

Set up alerts for:
- Webhook server downtime
- Failed SDK generations
- Failed documentation updates
- Package publishing failures

Example Slack webhook notification:

```bash
curl -X POST -H 'Content-type: application/json' \
  --data '{"text":"🚨 Sterling SDK generation failed for Chelsea API"}' \
  $SLACK_WEBHOOK_URL
```

## Troubleshooting

### Common Issues

1. **Webhook not triggering**
   - Check webhook delivery in GitHub settings
   - Verify webhook secret matches
   - Check server logs for errors

2. **SDK generation fails**
   - Verify OpenAPI spec is valid
   - Check API keys are set correctly
   - Review Sterling logs for errors

3. **Documentation sync fails**
   - Verify GitHub token has write access to vers-docs
   - Check if vers-docs repository exists
   - Review pull request creation logs

### Debug Commands

```bash
# Test webhook locally
curl -X POST http://localhost:8080/webhook/chelsea \
  -H "Content-Type: application/json" \
  -H "X-GitHub-Event: push" \
  -d @test-webhook-payload.json

# Test SDK generation manually
./zig-out/bin/sterling generate \
  --spec https://raw.githubusercontent.com/hdresearch/chelsea/main/openapi.yaml \
  --config sterling.toml \
  --enhance \
  --verbose

# Test documentation sync
./sync-vers-docs.sh
```

## Scaling Considerations

### High-Volume APIs

For APIs with frequent changes:

1. **Rate Limiting**: Implement rate limiting to avoid overwhelming GitHub API
2. **Queuing**: Use a job queue (Redis/RabbitMQ) for processing webhooks
3. **Caching**: Cache OpenAPI specs to avoid redundant processing
4. **Parallel Processing**: Generate SDKs for different languages in parallel

### Multiple APIs

For multiple API projects:

1. **Multi-tenant Configuration**: Support multiple `sterling.toml` configs
2. **Namespace Isolation**: Separate output directories per API
3. **Resource Isolation**: Use separate webhook endpoints per API

## Maintenance

### Regular Tasks

1. **Update Sterling**: Keep Sterling updated with latest features
2. **Rotate Secrets**: Regularly rotate API keys and webhook secrets
3. **Monitor Disk Usage**: Clean up old generated files
4. **Review Logs**: Check for errors and performance issues

### Backup Strategy

1. **Configuration Backup**: Version control all Sterling configurations
2. **Generated Code Backup**: SDK repositories serve as backups
3. **Documentation Backup**: vers-docs repository serves as backup

This deployment guide ensures reliable, secure, and scalable automation of SDK generation and documentation updates for the hdresearch ecosystem.
