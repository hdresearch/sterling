# Sterling - OpenAPI SDK Generator

Sterling is a powerful, open-source OpenAPI SDK generator written in Zig that creates production-ready SDKs across multiple programming languages. It's designed as a modern alternative to commercial solutions like Stainless, with AI-powered code enhancement and comprehensive language support.

## 🚀 Supported Languages

Sterling currently generates high-quality SDKs for **5 programming languages**:

| Language | Status | HTTP Client | Type System | Package Manager |
|----------|--------|-------------|-------------|-----------------|
| **TypeScript** | ✅ Production Ready | fetch API | Full TypeScript types | npm/yarn/pnpm |
| **Rust** | ✅ Production Ready | reqwest | serde models | Cargo/crates.io |
| **Python** | ✅ Production Ready | httpx/requests | Pydantic models | pip/PyPI |
| **Go** | ✅ Production Ready | net/http | struct types | Go modules |
| **Zig** | ✅ Production Ready | std.http | comptime types | Zig package manager |

## 🎯 Core Features

## 📚 Documentation

- **[Setup Guide](docs/GITHUB_SETUP_INSTRUCTIONS.md)** - Comprehensive installation and configuration guide
- **[Quick Reference](docs/QUICK_REFERENCE.md)** - Essential commands and configuration templates
- **[Documentation Index](docs/README.md)** - Complete documentation overview
- **[Examples](examples/)** - Sample configurations and OpenAPI specifications

### Multi-Language SDK Generation
- **Complete OpenAPI 3.0 support** - Full specification compliance
- **Type-safe code generation** - Strong typing across all target languages
- **Async/await patterns** - Modern asynchronous programming support
- **Authentication handling** - API keys, OAuth, custom auth schemes
- **Error handling & retries** - Robust error management with configurable retry logic
- **Request/response validation** - Runtime validation against OpenAPI schemas

### 🤖 AI-Powered Enhancement
- **LLM integration** with Claude 3.5 Sonnet for code improvement
- **Intelligent error fixing** - AI-powered debugging and optimization
- **Code quality enhancement** - Automatic style and pattern improvements
- **Documentation generation** - AI-enhanced API documentation

### 📦 Package Management & Distribution
- **Automatic repository creation** - GitHub integration for SDK repositories
- **Package publishing** - Automated publishing to language-specific registries
- **Version management** - Semantic versioning and release automation
- **Documentation sites** - Mintlify-powered documentation generation

## 🚀 Quick Start

### 1. Installation

```bash
# Build from source (requires Zig 0.12+)
git clone https://github.com/your-org/sterling
cd sterling
zig build -Doptimize=ReleaseFast

# Or download pre-built binary
curl -L https://github.com/your-org/sterling/releases/latest/download/sterling-linux-x64 -o sterling
chmod +x sterling
```

### 2. Configuration

Create a `sterling.toml` configuration file:

```toml
[project]
name = "my-api"
version = "1.0.0"
description = "My API SDK"

# Generate TypeScript SDK
[targets.typescript]
language = "typescript"
repository = "https://github.com/my-org/typescript-sdk"
output_dir = "./generated/typescript"
package_name = "@my-org/api-sdk"

# Generate Rust SDK
[targets.rust]
language = "rust"
repository = "https://github.com/my-org/rust-sdk"
output_dir = "./generated/rust"
package_name = "my-api-sdk"

# Generate Python SDK
[targets.python]
language = "python"
repository = "https://github.com/my-org/python-sdk"
output_dir = "./generated/python"
package_name = "my-api-client"

# Generate Go SDK
[targets.go]
language = "go"
repository = "https://github.com/my-org/go-sdk"
output_dir = "./generated/go"
module_name = "github.com/my-org/go-sdk"

# Generate Zig SDK
[targets.zig]
language = "zig"
repository = "https://github.com/my-org/zig-sdk"
output_dir = "./generated/zig"

# Optional: AI enhancement
[llm]
provider = "anthropic"
api_key = "sk-..."
model = "claude-3-5-sonnet-20241022"
```

### 3. Generate SDKs

```bash
# Basic generation
sterling generate --spec api.yaml --config sterling.toml

# With AI enhancement
export ANTHROPIC_API_KEY=your_key_here
sterling generate --spec api.yaml --config sterling.toml --enhance

# Generate specific language only
sterling generate --spec api.yaml --config sterling.toml --target typescript
```

## 📋 Language-Specific Features

### TypeScript SDK
- **Modern ES modules** with CommonJS compatibility
- **Full TypeScript definitions** for all API endpoints
- **Fetch-based HTTP client** with configurable options
- **Tree-shakeable exports** for optimal bundle size
- **Built-in request/response validation**

### Rust SDK
- **Async/await support** with tokio runtime
- **reqwest HTTP client** with connection pooling
- **serde serialization** for type-safe JSON handling
- **Comprehensive error types** with proper error chains
- **Optional features** for different HTTP backends

### Python SDK
- **Async and sync clients** for maximum flexibility
- **httpx HTTP client** with HTTP/2 support
- **Pydantic models** for request/response validation
- **Type hints** for full IDE support
- **Automatic retry logic** with exponential backoff

### Go SDK
- **Context-aware requests** for proper cancellation
- **Standard library HTTP client** with custom transport
- **Struct-based models** with JSON tags
- **Interface-based design** for easy testing
- **Comprehensive error handling**

### Zig SDK
- **Compile-time type safety** with comptime validation
- **std.http client** for minimal dependencies
- **JSON parsing/serialization** with built-in types
- **Memory-safe operations** with allocator patterns
- **Cross-platform compatibility**

## 🔧 Advanced Configuration

### Custom Templates
Override default templates for any language:

```toml
[targets.typescript]
language = "typescript"
template_dir = "./custom-templates/typescript"
```

### Authentication Configuration
```toml
[auth]
type = "bearer"  # or "api_key", "oauth2", "custom"
header = "Authorization"
prefix = "Bearer "
```

## 🤝 Contributing

Sterling is open source and welcomes contributions! Here's how to get started:

1. **Fork the repository**
2. **Set up development environment**:
   ```bash
   git clone https://github.com/your-username/sterling
   cd sterling
   zig build test  # Run tests
   ```
3. **Make your changes** and add tests
4. **Submit a pull request**

### Development Requirements
- Zig 0.12 or later
- Git for version control
- Optional: Anthropic API key for LLM features

## 📈 Roadmap

### Upcoming Language Support
- **Java** - Spring Boot compatible SDKs
- **C#** - .NET compatible with NuGet packaging
- **PHP** - Composer-ready with PSR standards
- **Ruby** - Gem-compatible with Rails integration
- **Swift** - iOS/macOS SDK generation

### Planned Features
- **GraphQL support** - Generate SDKs from GraphQL schemas
- **gRPC integration** - Protocol buffer to SDK generation
- **Mock server generation** - Automatic test server creation
- **SDK testing framework** - Automated SDK validation
- **Performance optimization** - Faster generation and smaller SDKs

## 📄 License

Sterling is licensed under the MIT License. See [LICENSE](LICENSE) for details.

## 🙏 Acknowledgments

- Built with [Zig](https://ziglang.org/) for performance and reliability
- Inspired by [Stainless](https://stainlessapi.com/) and other commercial SDK generators
- AI enhancement powered by [Anthropic Claude](https://anthropic.com/)
- Documentation generation using [Mintlify](https://mintlify.com/)

---

**Ready to generate your first SDK?** Check out our [examples](examples/) directory for complete working configurations!
