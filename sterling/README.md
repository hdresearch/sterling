# Sterling - OpenAPI SDK Generator

Sterling is an open source replacement for Stainless, written in Zig. It generates SDKs across multiple programming languages from OpenAPI specifications.

## Overview

Sterling transforms your OpenAPI specifications into production-ready SDKs for multiple programming languages, with optional AI assistance for polishing and error handling.

```mermaid
graph TD
    A[OpenAPI Spec] --> B[Sterling Generator]
    B --> C[TypeScript SDK]
    B --> D[Rust SDK]
    B --> E[Python SDK]
    B --> F[Go SDK]
    B --> G[Documentation]
    
    H[Configuration] --> B
    I[LLM Integration] --> B
    
    C --> J[GitHub Repo]
    D --> K[GitHub Repo]
    E --> L[GitHub Repo]
    F --> M[GitHub Repo]
    G --> N[Docs Site]
```

## Core Workflow: OpenAPI to Multi-Language SDKs

The following diagram illustrates Sterling's primary function - converting OpenAPI specifications into multiple language-specific SDKs:

```mermaid
flowchart LR
    subgraph "Input"
        A[OpenAPI 3.0 Spec<br/>api.yaml]
        B[Sterling Config<br/>sterling.toml]
    end
    
    subgraph "Sterling Engine"
        C[Parser & Validator]
        D[Schema Analyzer]
        E[Code Generator]
        F[Template Engine]
        G[LLM Enhancer]
    end
    
    subgraph "Generated SDKs"
        H[TypeScript SDK<br/>📦 npm package]
        I[Rust SDK<br/>📦 crates.io]
        J[Python SDK<br/>📦 PyPI package]
        K[Go SDK<br/>📦 Go module]
        L[API Documentation<br/>📚 Mintlify docs]
    end
    
    A --> C
    B --> E
    C --> D
    D --> E
    E --> F
    F --> G
    
    G --> H
    G --> I
    G --> J
    G --> K
    G --> L
    
    style A fill:#e1f5fe
    style H fill:#c8e6c9
    style I fill:#ffcdd2
    style J fill:#fff3e0
    style K fill:#e8f5e8
    style L fill:#f3e5f5
```

## Architecture

Sterling follows a modular architecture that separates parsing, generation, and output handling:

```mermaid
graph TB
    subgraph "Input Layer"
        A[OpenAPI 3.0 Spec]
        B[Sterling Config]
        C[Custom Templates]
    end
    
    subgraph "Core Engine"
        D[OpenAPI Parser]
        E[Schema Validator]
        F[AST Generator]
        G[Code Generator]
        H[Template Engine]
    end
    
    subgraph "Language Generators"
        I[TypeScript Generator<br/>• Fetch-based HTTP<br/>• TypeScript types<br/>• ESM/CJS support]
        J[Rust Generator<br/>• reqwest HTTP<br/>• serde models<br/>• async/await]
        K[Python Generator<br/>• httpx/requests<br/>• Pydantic models<br/>• async support]
        L[Go Generator<br/>• net/http<br/>• struct types<br/>• context support]
    end
    
    subgraph "AI Enhancement"
        M[LLM Integration]
        N[Code Review]
        O[Error Handling]
        P[Documentation Polish]
    end
    
    subgraph "Output Layer"
        Q[File System]
        R[Git Repositories]
        S[Package Registries]
        T[Documentation Sites]
    end
    
    A --> D
    B --> G
    C --> H
    
    D --> E
    E --> F
    F --> G
    G --> H
    
    H --> I
    H --> J
    H --> K
    H --> L
    
    G --> M
    M --> N
    N --> O
    O --> P
    
    I --> Q
    J --> Q
    K --> Q
    L --> Q
    
    Q --> R
    R --> S
    Q --> T
    
    style A fill:#e1f5fe
    style I fill:#1976d2,color:#fff
    style J fill:#d84315,color:#fff
    style K fill:#388e3c,color:#fff
    style L fill:#0277bd,color:#fff
    style M fill:#ff9800,color:#fff
```

## Features

### Multi-Language Support
- **TypeScript**: Full type safety, ESM/CJS, Node.js + web
- **Rust**: async/await, serde integration, reqwest HTTP client
- **Python**: Pydantic models, httpx/requests, async support
- **Go**: Context support, standard library HTTP, struct types

### AI-Enhanced Generation
- LLM integration for code improvement
- Intelligent error handling patterns
- Documentation enhancement
- Code review and optimization

### Enterprise Ready
- GitHub repository automation
- Package registry publishing
- Comprehensive documentation generation
- CI/CD integration

## Installation

### From Source
```bash
git clone https://github.com/your-org/sterling
cd sterling
zig build -Doptimize=ReleaseFast
```

### Binary Download
Download the latest release from the [releases page](https://github.com/your-org/sterling/releases).

## Quick Start

1. **Create a configuration file** (`sterling.toml`):
```toml
[project]
name = "my-api"
version = "1.0.0"
description = "My API SDK"

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
model = "claude-3-sonnet-20240229"
```

2. **Generate SDKs**:
```bash
sterling generate --spec api.yaml --config sterling.toml
```

3. **Deploy** (optional):
```bash
sterling deploy --config sterling.toml
```

## Configuration

Sterling uses TOML configuration files to define generation targets and options:

```toml
[project]
name = "petstore-api"
version = "1.0.0"
description = "Pet Store API SDK"
author = "Your Organization"
license = "MIT"

[targets.typescript]
language = "typescript"
repository = "https://github.com/org/typescript-sdk"
output_dir = "./generated/typescript"
package_name = "@petstore/sdk"

[targets.rust]
language = "rust"
repository = "https://github.com/org/rust-sdk"
output_dir = "./generated/rust"
package_name = "petstore-sdk"

[targets.python]
language = "python"
repository = "https://github.com/org/python-sdk"
output_dir = "./generated/python"

[targets.go]
language = "go"
repository = "https://github.com/org/go-sdk"
output_dir = "./generated/go"

[llm]
provider = "anthropic"
api_key = "sk-..."
model = "claude-3-sonnet-20240229"

[output.docs]
format = "mintlify"
repository = "https://github.com/org/docs"
output_dir = "./generated/docs"
```

## Repository Structure

```
sterling/
├── src/                    # Core Sterling implementation
│   ├── main.zig           # CLI entry point
│   ├── parser/            # OpenAPI parsing
│   ├── generator/         # Code generation
│   ├── languages/         # Language-specific generators
│   └── llm/              # LLM integration
├── templates/             # Code generation templates
├── examples/              # Example configurations
├── build.zig             # Zig build configuration
└── README.md             # This file
```

## Getting Started

1. **Install Sterling** (build from source or download binary)
2. **Prepare your OpenAPI spec** (`api.yaml` or `api.json`)
3. **Configure targets** in `sterling.toml`
4. **Generate SDKs**: `sterling generate --spec api.yaml --config sterling.toml`
5. **Deploy**: Sterling automatically pushes to configured GitHub repositories

## Example Output

From a simple Pet Store API specification, Sterling generates:

```mermaid
graph LR
    A[petstore.yaml<br/>OpenAPI 3.0 Spec] --> B[Sterling Generator]
    
    B --> C[TypeScript SDK<br/>📦 @petstore/sdk]
    B --> D[Rust SDK<br/>📦 petstore-sdk]
    B --> E[Python SDK<br/>📦 petstore-client]
    B --> F[Go SDK<br/>📦 github.com/org/petstore-go]
    B --> G[API Documentation<br/>📚 docs.petstore.com]
    
    C --> H[npm registry<br/>npm install @petstore/sdk]
    D --> I[crates.io<br/>cargo add petstore-sdk]
    E --> J[PyPI<br/>pip install petstore-client]
    F --> K[Go modules<br/>go get github.com/org/petstore-go]
    G --> L[Mintlify docs site<br/>Auto-deployed]
    
    style A fill:#e3f2fd
    style C fill:#1976d2,color:#fff
    style D fill:#d84315,color:#fff
    style E fill:#388e3c,color:#fff
    style F fill:#0277bd,color:#fff
    style G fill:#7b1fa2,color:#fff
```

Each generated SDK includes:
- Type-safe request/response models
- Authentication handling
- Error handling and retries
- Comprehensive documentation
- Usage examples and tests

## Contributing

Sterling is open source and welcomes contributions. Please see our [contributing guidelines](CONTRIBUTING.md) for details on:

- Setting up the development environment
- Code style and conventions
- Testing requirements
- Pull request process

## License

This project is licensed under the MIT License - see the LICENSE file for details.
