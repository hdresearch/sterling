# Sterling Automation Setup for hdresearch/chelsea

This setup enables automatic SDK generation and documentation updates when OpenAPI specifications change in the hdresearch/chelsea repository.

## Architecture

```
hdresearch/chelsea (OpenAPI changes)
        ↓ (webhook trigger)
Sterling SDK Generator
        ↓ (generates)
├── TypeScript SDK → hdresearch/chelsea-typescript-sdk
├── Rust SDK → hdresearch/chelsea-rust-sdk  
├── Python SDK → hdresearch/chelsea-python-sdk
├── Go SDK → hdresearch/chelsea-go-sdk
└── Documentation → hdresearch/vers-docs (PR)
```

## Setup Steps

1. **Configure Environment Variables**
   ```bash
   cp .env.example .env
   # Edit .env with your actual API keys and tokens
   ```

2. **Build Sterling**
   ```bash
   zig build -Doptimize=ReleaseFast
   ```

3. **Test Manual Generation**
   ```bash
   # Download Chelsea OpenAPI spec
   curl -o chelsea-openapi.yaml https://raw.githubusercontent.com/hdresearch/chelsea/main/openapi.yaml
   
   # Generate SDKs
   ./zig-out/bin/sterling generate --spec chelsea-openapi.yaml --config sterling.toml --enhance
   ```

4. **Set Up Webhook (for automation)**
   - Deploy webhook server to receive GitHub webhooks
   - Configure webhook in hdresearch/chelsea repository settings
   - Point to your Sterling webhook endpoint

## Manual Workflow

```bash
# 1. Generate SDKs from OpenAPI spec
./zig-out/bin/sterling generate --spec openapi.yaml --config sterling.toml --enhance

# 2. Sync documentation with vers-docs
./sync-vers-docs.sh
```

## Automated Workflow

When OpenAPI files change in hdresearch/chelsea:
1. GitHub webhook triggers Sterling
2. Sterling downloads the updated OpenAPI spec
3. Generates enhanced SDKs for all languages
4. Creates/updates SDK repositories
5. Generates documentation
6. Creates PR in hdresearch/vers-docs

## Configuration

The `sterling.toml` file configures:
- Source repository (hdresearch/chelsea)
- Target SDK repositories
- Documentation output (hdresearch/vers-docs)
- LLM enhancement settings
- GitHub automation settings

## Features

- **Multi-language SDK generation**: TypeScript, Rust, Python, Go
- **LLM enhancement**: AI-powered code improvement and documentation
- **GitHub automation**: Automatic repository creation and CI/CD setup
- **Documentation generation**: Mintlify-compatible docs for vers-docs
- **Package publishing**: Automatic publishing to npm, PyPI, crates.io
- **Webhook integration**: Automatic triggering on OpenAPI changes

## Monitoring

Check the following for automation status:
- GitHub Actions in this repository
- SDK repository commits and releases
- Pull requests in hdresearch/vers-docs
- Package registry publications
