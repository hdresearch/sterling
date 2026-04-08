# Sterling Documentation

Welcome to the Sterling documentation! Sterling is an OpenAPI SDK generator that creates production-ready SDKs for multiple programming languages.

## Documentation Index

### Getting Started
- **[GitHub Setup Instructions](GITHUB_SETUP_INSTRUCTIONS.md)** - Comprehensive guide for setting up Sterling with GitHub, including installation, configuration, development workflow, and contribution guidelines
- **[Quick Reference](QUICK_REFERENCE.md)** - Essential commands, configuration templates, and common workflows

### Core Documentation
- **[Main README](../README.md)** - Project overview and basic usage
- **[Configuration Guide](../sterling.toml)** - Example configuration file
- **[Examples](../examples/)** - Sample OpenAPI specifications and configurations

## Quick Links

### Essential Commands
```bash
# Build Sterling
zig build -Doptimize=ReleaseFast

# Generate SDKs
./zig-out/bin/sterling generate openapi.yaml

# Run tests
zig build test
```

### Supported Languages
- **TypeScript** - fetch API, npm packaging
- **Rust** - reqwest, Cargo integration  
- **Python** - httpx, pip/PyPI publishing
- **Go** - net/http, Go modules
- **Zig** - std.http, package manager

### Key Features
- Multi-language SDK generation
- AI-powered code enhancement
- GitHub repository automation
- Package publishing integration
- Comprehensive templating system

---

For detailed setup instructions, see [GitHub Setup Instructions](GITHUB_SETUP_INSTRUCTIONS.md).
For quick command reference, see [Quick Reference](QUICK_REFERENCE.md).
