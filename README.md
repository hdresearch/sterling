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
    
    M --> N
    M --> O
    M --> P
    N --> G
    O --> G
    P --> H
    
    I --> Q
    J --> Q
    K --> Q
    L --> Q
    
    Q --> R
    Q --> S
    Q --> T
```

## SDK Generation Flow

The following sequence diagram shows how Sterling processes your OpenAPI specification through to final SDK deployment:

```mermaid
sequenceDiagram
    participant User
    participant Sterling
    participant Parser
    participant Generator
    participant LLM
    participant GitHub
    participant Registry
    
    User->>Sterling: sterling generate --spec api.yaml
    Sterling->>Parser: Load & validate OpenAPI spec
    Parser->>Parser: Parse endpoints, models, auth
    Parser->>Generator: Provide parsed AST
    
    loop For each target language
        Generator->>Generator: Generate base SDK code
        Generator->>LLM: Request code review & enhancement
        LLM->>Generator: Return improved code + docs
        Generator->>GitHub: Push to language-specific repo
        GitHub->>Registry: Trigger package publication
        Registry-->>User: SDK available for installation
    end
    
    Generator->>GitHub: Generate & deploy documentation
    Sterling->>User: ✅ Generation complete
    
    Note over User,Registry: SDKs are now available:<br/>npm install @org/api-sdk<br/>pip install org-api-sdk<br/>cargo add org-api-sdk<br/>go get github.com/org/go-sdk
```

## Multi-Language Support Matrix

Sterling generates idiomatic code for each target language, respecting language-specific conventions and best practices:

```mermaid
graph TB
    subgraph "OpenAPI Features"
        A[REST Endpoints]
        B[Authentication<br/>• API Key<br/>• OAuth 2.0<br/>• Bearer Token]
        C[Request/Response Models]
        D[Error Handling]
        E[File Uploads]
        F[Webhooks]
    end
    
    subgraph "TypeScript SDK"
        G[• Fetch-based HTTP client<br/>• Full TypeScript types<br/>• ESM + CommonJS<br/>• Tree-shakeable<br/>• Browser + Node.js]
    end
    
    subgraph "Rust SDK"
        H[• reqwest HTTP client<br/>• serde serialization<br/>• async/await support<br/>• Error types<br/>• tokio runtime]
    end
    
    subgraph "Python SDK"
        I[• httpx async client<br/>• Pydantic models<br/>• Type hints<br/>• asyncio support<br/>• requests fallback]
    end
    
    subgraph "Go SDK"
        J[• net/http client<br/>• Struct types<br/>• Context support<br/>• Interface-based<br/>• Generics (Go 1.18+)]
    end
    
    A --> G
    A --> H
    A --> I
    A --> J
    
    B --> G
    B --> H
    B --> I
    B --> J
    
    C --> G
    C --> H
    C --> I
    C --> J
    
    D --> G
    D --> H
    D --> I
    D --> J
    
    E --> G
    E --> H
    E --> I
    E --> J
    
    F --> G
    F --> H
    F --> I
    F --> J
    
    style G fill:#3178c6,color:#fff
    style H fill:#ce422b,color:#fff
    style I fill:#3776ab,color:#fff
    style J fill:#00add8,color:#fff
```

## Configuration-Driven Generation

Sterling uses a declarative configuration approach to define how SDKs should be generated and where they should be published:

```mermaid
graph LR
    subgraph "sterling.toml Configuration"
        A[Target Languages<br/>typescript, rust,<br/>python, go]
        B[Repository URLs<br/>GitHub destinations<br/>for each SDK]
        C[LLM Settings<br/>Provider, model,<br/>enhancement options]
        D[Output Options<br/>Documentation format,<br/>package settings]
    end
    
    subgraph "Sterling Processing"
        E[Configuration Parser]
        F[Target Resolver]
        G[Generator Orchestrator]
    end
    
    subgraph "Generated Outputs"
        H[TypeScript SDK<br/>→ github.com/org/ts-sdk]
        I[Rust SDK<br/>→ github.com/org/rust-sdk]
        J[Python SDK<br/>→ github.com/org/py-sdk]
        K[Go SDK<br/>→ github.com/org/go-sdk]
        L[Documentation<br/>→ docs.example.com]
    end
    
    A --> E
    B --> F
    C --> G
    D --> G
    
    E --> G
    F --> G
    
    G --> H
    G --> I
    G --> J
    G --> K
    G --> L
```

## LLM-Enhanced Code Generation

Sterling integrates with Large Language Models to enhance generated code quality and add intelligent error handling:

```mermaid
flowchart TD
    subgraph "Base Generation"
        A[OpenAPI Schema] --> B[Template-Based<br/>Code Generation]
        B --> C[Raw SDK Code]
    end
    
    subgraph "LLM Enhancement Pipeline"
        D[Code Review Agent]
        E[Error Handling<br/>Enhancer]
        F[Documentation<br/>Generator]
        G[Best Practices<br/>Validator]
    end
    
    subgraph "Enhanced Output"
        H[Production-Ready SDK<br/>• Robust error handling<br/>• Comprehensive docs<br/>• Idiomatic patterns<br/>• Type safety]
    end
    
    C --> D
    D --> E
    E --> F
    F --> G
    G --> H
    
    I[LLM Provider<br/>Claude, GPT-4, etc.] -.-> D
    I -.-> E
    I -.-> F
    I -.-> G
    
    style I fill:#ff9800,color:#fff
    style H fill:#4caf50,color:#fff
```

## Features

- **Multi-language SDK generation** (TypeScript, Rust, Python, Go)
- **Support for various authentication methods** (API Key, OAuth, Bearer Token)
- **Configurable output** to different GitHub repositories
- **Documentation generation** (Mintlify compatible)
- **LLM integration** for error handling and final touches
- **Deterministic builds** with optional AI assistance

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

This repository contains two main projects:

```mermaid
graph TD
    A[Repository Root] --> B[sterling/]
    A --> C[browser/]
    
    B --> D[Sterling SDK Generator]
    C --> E[Lightpanda Browser]
    
    D --> F[OpenAPI → Multi-language SDKs]
    E --> G[Headless browser for AI agents]
    
    F --> H[TypeScript, Rust, Python, Go]
    G --> I[Fast, low-memory automation]
```

### Sterling SDK Generator (`./sterling/`)
- OpenAPI specification parser and validator
- Multi-language code generators
- LLM integration for code improvement
- GitHub repository management
- Documentation generation

### Lightpanda Browser (`./browser/`)
- Headless browser built from scratch in Zig
- Ultra-low memory footprint (16x less than Chrome)
- Exceptionally fast execution (9x faster than Chrome)
- Compatible with Playwright, Puppeteer, chromedp through CDP

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

Sterling is open source and welcomes contributions. See the individual project directories for specific contribution guidelines.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
