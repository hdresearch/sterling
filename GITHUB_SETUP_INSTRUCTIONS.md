# Sterling Project GitHub Setup Instructions

## Overview
Sterling is an OpenAPI SDK generator written in Zig that creates production-ready SDKs for multiple programming languages (TypeScript, Rust, Python, Go, and Zig). This guide will help you set up the Sterling project with GitHub for development and collaboration.

## Prerequisites

### Required Software
- **Zig 0.15.2+** - Download from [ziglang.org](https://ziglang.org/download/)
- **Git** - For version control
- **GitHub account** - For repository hosting

### Optional (for AI features)
- **Anthropic API key** - For LLM-powered code enhancement
- **GitHub Personal Access Token** - For automated repository creation

## Initial Setup

### 1. Clone the Repository
```bash
# Clone the Sterling repository
git clone https://github.com/hdresearch/sterling.git
cd sterling

# Check that you're on the main branch
git branch -a
```

### 2. Verify Project Structure
The project should contain:
```
sterling/
├── src/                    # Core Sterling source code
├── templates/              # SDK generation templates
├── examples/               # Example configurations
├── tests/                  # Test suite
├── build.zig              # Zig build configuration
├── build.zig.zon          # Zig package manifest
├── sterling.toml          # Sterling configuration
├── README.md              # Project documentation
└── .gitignore             # Git ignore rules
```

### 3. Build and Test Sterling
```bash
# Build Sterling in release mode
zig build -Doptimize=ReleaseFast

# Run the test suite
zig build test

# Verify the build works
./zig-out/bin/sterling --help
```

## Configuration

### 1. Sterling Configuration (`sterling.toml`)
The main configuration file defines SDK generation targets:

```toml
[project]
name = "your-api"
version = "1.0.0"
description = "Your API SDK"

# TypeScript SDK target
[targets.typescript]
language = "typescript"
repository = "https://github.com/your-org/typescript-sdk"
output_dir = "./generated/typescript"
branch = "main"

# Rust SDK target
[targets.rust]
language = "rust"
repository = "https://github.com/your-org/rust-sdk"
output_dir = "./generated/rust"
branch = "main"

# Python SDK target
[targets.python]
language = "python"
repository = "https://github.com/your-org/python-sdk"
output_dir = "./generated/python"
branch = "main"

# Go SDK target
[targets.go]
language = "go"
repository = "https://github.com/your-org/go-sdk"
output_dir = "./generated/go"
branch = "main"

# Zig SDK target
[targets.zig]
language = "zig"
repository = "https://github.com/your-org/zig-sdk"
output_dir = "./generated/zig"
branch = "main"
```

### 2. Environment Variables
Set up optional environment variables for enhanced features:

```bash
# For AI-powered code enhancement
export ANTHROPIC_API_KEY="your-anthropic-api-key"

# For automated GitHub repository creation
export GITHUB_TOKEN="your-github-personal-access-token"
```

## Development Workflow

### 1. Making Changes
```bash
# Create a feature branch
git checkout -b feature/your-feature-name

# Make your changes to the codebase
# Edit files in src/, templates/, etc.

# Test your changes
zig build test

# Build to ensure everything compiles
zig build -Doptimize=ReleaseFast
```

### 2. Testing SDK Generation
```bash
# Test with the example OpenAPI spec
./zig-out/bin/sterling generate examples/petstore.yaml

# Check generated SDKs
ls -la generated/
```

### 3. Committing Changes
```bash
# Add all changes
git add .

# Commit with a descriptive message
git commit -m "feat: add new SDK generation feature"

# Push to your branch
git push origin feature/your-feature-name
```

### 4. Creating Pull Requests
1. Push your feature branch to GitHub
2. Open a pull request against the `main` branch
3. Provide a clear description of your changes
4. Wait for code review and CI checks

## GitHub Repository Setup

### 1. Fork the Repository (for contributors)
1. Go to https://github.com/hdresearch/sterling
2. Click "Fork" to create your own copy
3. Clone your fork:
   ```bash
   git clone https://github.com/YOUR-USERNAME/sterling.git
   cd sterling
   git remote add upstream https://github.com/hdresearch/sterling.git
   ```

### 2. Set Up GitHub Actions (for maintainers)
Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [ main ]
  pull_request:
    branches: [ main ]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v4
    
    - name: Setup Zig
      uses: goto-bus-stop/setup-zig@v2
      with:
        version: 0.15.2
    
    - name: Build
      run: zig build -Doptimize=ReleaseFast
    
    - name: Test
      run: zig build test
    
    - name: Test SDK Generation
      run: |
        ./zig-out/bin/sterling generate examples/petstore.yaml
        ls -la generated/
```

### 3. Release Management
For creating releases:

```bash
# Tag a new version
git tag -a v1.0.0 -m "Release version 1.0.0"
git push origin v1.0.0

# GitHub will automatically create a release
# Add release notes and attach binaries if needed
```

## Using Sterling

### 1. Basic SDK Generation
```bash
# Generate SDKs from an OpenAPI spec
./zig-out/bin/sterling generate path/to/openapi.yaml

# Generate for specific languages only
./zig-out/bin/sterling generate --languages typescript,rust path/to/openapi.yaml

# Use custom configuration
./zig-out/bin/sterling generate --config custom-sterling.toml path/to/openapi.yaml
```

### 2. Advanced Features
```bash
# Enable AI-powered enhancement (requires ANTHROPIC_API_KEY)
./zig-out/bin/sterling generate --enhance path/to/openapi.yaml

# Automatically create GitHub repositories for SDKs (requires GITHUB_TOKEN)
./zig-out/bin/sterling generate --create-repos path/to/openapi.yaml

# Generate with custom templates
./zig-out/bin/sterling generate --template-dir ./custom-templates path/to/openapi.yaml
```

## Project Structure Details

### Core Components
- **`src/main.zig`** - Main entry point and CLI
- **`src/generator/`** - SDK generation logic
- **`src/parser/`** - OpenAPI specification parsing
- **`src/templates/`** - Template engine for code generation
- **`src/github/`** - GitHub integration for repository management
- **`src/llm/`** - AI enhancement integration

### Templates
- **`templates/typescript/`** - TypeScript SDK templates
- **`templates/rust/`** - Rust SDK templates
- **`templates/python/`** - Python SDK templates
- **`templates/go/`** - Go SDK templates
- **`templates/zig/`** - Zig SDK templates

### Examples
- **`examples/petstore.yaml`** - Sample OpenAPI specification
- **`examples/configs/`** - Example Sterling configurations

## Troubleshooting

### Common Issues

1. **Zig version mismatch**
   ```bash
   # Check Zig version
   zig version
   # Should be 0.15.2 or later
   ```

2. **Build failures**
   ```bash
   # Clean build cache
   rm -rf zig-cache zig-out
   zig build -Doptimize=ReleaseFast
   ```

3. **Missing dependencies**
   ```bash
   # Fetch dependencies
   zig build --fetch
   ```

4. **GitHub authentication issues**
   ```bash
   # Check GitHub token permissions
   curl -H "Authorization: token $GITHUB_TOKEN" https://api.github.com/user
   ```

### Getting Help
- **Issues**: Report bugs at https://github.com/hdresearch/sterling/issues
- **Discussions**: Ask questions in GitHub Discussions
- **Documentation**: Check the README.md and examples/

## Contributing

### Code Style
- Follow Zig standard formatting: `zig fmt src/`
- Write tests for new features
- Update documentation for user-facing changes
- Keep commits atomic and well-described

### Testing
```bash
# Run all tests
zig build test

# Run specific test
zig build test -- --filter "test_name"

# Test SDK generation
./zig-out/bin/sterling generate examples/petstore.yaml
```

### Documentation
- Update README.md for major changes
- Add examples for new features
- Document configuration options in sterling.toml
- Include code comments for complex logic

---

This setup guide should get you started with Sterling development. For more detailed information, see the project README.md and examples directory.
