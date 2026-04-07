# Sterling - OpenAPI SDK Generator

Sterling is an open source replacement for Stainless, written in Zig. It generates SDKs across multiple programming languages from OpenAPI specifications.

## Features
- Multi-language SDK generation (TypeScript, Rust, Python, Go)
- Support for various authentication methods (API Key, OAuth, Bearer Token)
- Configurable output to different GitHub repositories
- Documentation generation (Mintlify compatible)
- LLM integration for error handling and final touches
- Deterministic builds with optional AI assistance

## Usage
```bash
sterling generate --spec api.yaml --config sterling.toml
```

## Configuration
Create a `sterling.toml` file:
```toml
[targets.typescript]
language = "typescript"
repository = "https://github.com/org/typescript-sdk"
output_dir = "./generated/typescript"

[targets.rust]
language = "rust"
repository = "https://github.com/org/rust-sdk"
output_dir = "./generated/rust"

[llm]
provider = "anthropic"
api_key = "sk-..."
model = "claude-3-sonnet-20240229"
```

