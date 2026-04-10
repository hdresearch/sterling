# Sterling SDK Generator - Complete Usage Guide

## Overview
Sterling is a production-ready, open-source SDK generator that serves as a modern alternative to Stainless. Written in Zig for performance, it generates high-quality SDKs across multiple programming languages from OpenAPI specifications.

## Features
- **Multi-language SDK generation**: TypeScript, Rust, Python, Go
- **LLM-powered enhancement**: Uses Anthropic Claude for code improvement
- **GitHub automation**: Automatic repository creation and CI/CD setup
- **Package publishing**: Automated publishing to npm, PyPI, crates.io
- **Documentation generation**: Mintlify-compatible documentation
- **Webhook integration**: Automatic triggering on OpenAPI changes

## Installation

### Prerequisites
- Zig 0.12+ (tested with 0.15.2)
- Git

### Build from Source
```bash
git clone https://github.com/hdresearch/sterling.git
cd sterling
zig build -Doptimize=ReleaseFast
```

## Quick Start

### 1. Initialize Configuration
```bash
./zig-out/bin/sterling init
```

This creates a `sterling.toml` configuration file with sensible defaults.

### 2. Generate SDKs
```bash
./zig-out/bin/sterling generate --spec openapi.yaml --config sterling.toml
```

### 3. Enable Advanced Features
```bash
# With LLM enhancement
export ANTHROPIC_API_KEY=your_key_here
./zig-out/bin/sterling generate --spec openapi.yaml --config sterling.toml --enhance

# With all features (requires environment variables)
export ANTHROPIC_API_KEY=your_key_here
export GITHUB_TOKEN=your_token_here
export NPM_TOKEN=your_npm_token_here
export PYPI_TOKEN=your_pypi_token_here
```

## Configuration

### Basic Configuration (sterling.toml)
```toml
[project]
name = "my-api"
description = "My API SDK"
version = "1.0.0"

[languages]
typescript = true
rust = true
python = true
go = true

[typescript]
package_name = "@myorg/my-api"
output_dir = "./generated/typescript"

[rust]
package_name = "my-api"
output_dir = "./generated/rust"

[python]
package_name = "my-api"
output_dir = "./generated/python"

[go]
package_name = "github.com/myorg/my-api-go"
output_dir = "./generated/go"

[github]
org = "myorg"
create_repos = true
setup_ci = true

[publishing]
npm = true
pypi = true
crates_io = true

[docs]
generate = true
format = "mintlify"
output_dir = "./generated/docs"
```

## Environment Variables

### Required for Advanced Features
- `ANTHROPIC_API_KEY`: For LLM code enhancement
- `GITHUB_TOKEN`: For GitHub repository automation
- `NPM_TOKEN`: For npm package publishing
- `PYPI_TOKEN`: For PyPI package publishing
- `CRATES_IO_TOKEN`: For crates.io publishing

### Optional
- `WEBHOOK_SECRET`: For webhook security
- `SLACK_WEBHOOK_URL`: For notifications

## Generated Output

Sterling generates complete, production-ready SDKs:

### TypeScript SDK
```
generated/typescript/
├── package.json
├── tsconfig.json
├── src/
│   ├── client.ts
│   ├── types.ts
│   └── index.ts
├── tests/
└── README.md
```

### Rust SDK
```
generated/rust/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── client.rs
│   └── types.rs
├── tests/
└── README.md
```

### Python SDK
```
generated/python/
├── setup.py
├── pyproject.toml
├── my_api/
│   ├── __init__.py
│   ├── client.py
│   └── types.py
├── tests/
└── README.md
```

### Go SDK
```
generated/go/
├── go.mod
├── client.go
├── types.go
├── client_test.go
└── README.md
```

## Chelsea Automation Setup

Sterling includes pre-configured automation for the hdresearch/chelsea API:

### 1. Use Pre-configured Setup
```bash
# Copy the Chelsea-specific configuration
cp sterling-chelsea-config.toml sterling.toml

# Run the automated setup script
./setup-chelsea-automation.sh
```

### 2. Manual Chelsea Setup
```bash
# Generate SDKs for Chelsea API
./zig-out/bin/sterling generate \
  --spec https://raw.githubusercontent.com/hdresearch/chelsea/main/openapi.yaml \
  --config sterling-chelsea-config.toml \
  --enhance

# Sync documentation with vers-docs
./sync-vers-docs.sh
```

### 3. Webhook Server for Automation
```bash
# Start webhook server for automatic updates
export WEBHOOK_SECRET=your_secret_here
./webhook-server.sh
```

## Advanced Usage

### Module-Level Access
For advanced users, Sterling's modules can be used directly in Zig code:

```zig
const sterling = @import("sterling");
const config = @import("config");

// Load configuration
const cfg = try config.loadConfig(allocator, "sterling.toml");

// Generate SDKs
var generator = sterling.SDKGenerator.init(allocator, spec, cfg);
try generator.generateAll();

// Apply LLM enhancement
var enhancer = sterling.LLMEnhancer.init(allocator, api_key);
try enhancer.enhanceGeneratedSDKs("./generated");

// Setup GitHub repositories
var github = sterling.GitHubAutomation.init(allocator, token, cfg);
try github.setupRepositories();
```

## Testing

### Run All Tests
```bash
zig build test
```

### Integration Tests
```bash
# Test complete functionality
zig test tests/integration/complete_test.zig

# Test specific modules
zig test tests/parser/openapi_test.zig
zig test tests/generator/typescript_test.zig
```

## Deployment

### Docker Deployment
```dockerfile
FROM alpine:latest
RUN apk add --no-cache zig
COPY . /app
WORKDIR /app
RUN zig build -Doptimize=ReleaseFast
EXPOSE 8080
CMD ["./zig-out/bin/sterling", "webhook", "--port", "8080"]
```

### Cloud Deployment
Sterling can be deployed on:
- Railway
- Render
- Fly.io
- AWS Lambda (with custom runtime)
- Google Cloud Run

## Troubleshooting

### Common Issues

1. **Build Errors**
   ```bash
   # Ensure Zig version compatibility
   zig version  # Should be 0.12+
   
   # Clean build cache
   rm -rf zig-cache .zig-cache
   zig build
   ```

2. **OpenAPI Parsing Errors**
   ```bash
   # Validate your OpenAPI spec
   npx @apidevtools/swagger-parser validate openapi.yaml
   ```

3. **LLM Enhancement Not Working**
   ```bash
   # Check API key
   echo $ANTHROPIC_API_KEY
   
   # Test API connectivity
   curl -H "x-api-key: $ANTHROPIC_API_KEY" https://api.anthropic.com/v1/messages
   ```

### Performance Tuning

1. **Faster Builds**
   ```bash
   # Use release mode for production
   zig build -Doptimize=ReleaseFast
   
   # Parallel generation (automatically detected)
   export ZIG_PARALLEL=8
   ```

2. **Memory Usage**
   ```bash
   # For large OpenAPI specs
   export ZIG_HEAP_SIZE=4GB
   ```

## Contributing

Sterling is open source and welcomes contributions:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass: `zig build test`
5. Submit a pull request

## License

Sterling is released under the MIT License. See LICENSE file for details.

## Support

- GitHub Issues: https://github.com/hdresearch/sterling/issues
- Documentation: https://docs.hdresearch.org/sterling
- Community: Join our Discord server

---

Sterling SDK Generator - Making SDK generation fast, reliable, and open source.
