# Sterling Quick Reference

## Essential Commands

### Setup
```bash
# Clone and build
git clone https://github.com/hdresearch/sterling.git
cd sterling
zig build -Doptimize=ReleaseFast

# Test installation
./zig-out/bin/sterling --help
```

### SDK Generation
```bash
# Basic generation
./zig-out/bin/sterling generate openapi.yaml

# Specific languages
./zig-out/bin/sterling generate --languages typescript,rust openapi.yaml

# With AI enhancement
export ANTHROPIC_API_KEY="your-key"
./zig-out/bin/sterling generate --enhance openapi.yaml

# Auto-create GitHub repos
export GITHUB_TOKEN="your-token"
./zig-out/bin/sterling generate --create-repos openapi.yaml
```

### Development
```bash
# Build and test
zig build test
zig build -Doptimize=ReleaseFast

# Format code
zig fmt src/

# Clean build
rm -rf zig-cache zig-out
```

### Git Workflow
```bash
# Feature branch
git checkout -b feature/name
# ... make changes ...
git add .
git commit -m "feat: description"
git push origin feature/name

# Update from main
git fetch upstream
git rebase upstream/main
```

## Configuration Template

```toml
[project]
name = "my-api"
version = "1.0.0"
description = "My API SDK"

[targets.typescript]
language = "typescript"
repository = "https://github.com/my-org/typescript-sdk"
output_dir = "./generated/typescript"

[targets.rust]
language = "rust"
repository = "https://github.com/my-org/rust-sdk"
output_dir = "./generated/rust"
```

## Supported Languages
- TypeScript (fetch API, npm)
- Rust (reqwest, Cargo)
- Python (httpx, pip)
- Go (net/http, modules)
- Zig (std.http, package manager)
