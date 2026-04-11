# Sterling

Open-source OpenAPI SDK generator written in Zig. Parses an OpenAPI 3.1 spec and generates typed HTTP client SDKs in TypeScript, Rust, Python, and Go.

Built as an alternative to [Stainless](https://stainlessapi.com/).

## Quick start

```bash
# Build (requires Zig 0.15.2+)
zig build

# Generate SDKs from an OpenAPI spec
./zig-out/bin/sterling generate \
  --spec openapi.json \
  --config sterling.toml

# With LLM enhancement (optional)
ANTHROPIC_API_KEY=sk-... ./zig-out/bin/sterling generate \
  --spec openapi.json \
  --config sterling.toml \
  --enhance
```

## Configuration

```toml
# sterling.toml
[project]
name = "chelsea"
version = "0.1.0"

[[targets]]
language = "typescript"
output_dir = "./generated/typescript"

[[targets]]
language = "rust"
output_dir = "./generated/rust"

[[targets]]
language = "python"
output_dir = "./generated/python"

[[targets]]
language = "go"
output_dir = "./generated/go"

[llm]
provider = "anthropic"
api_key = "${ANTHROPIC_API_KEY}"
model = "claude-sonnet-4-20250514"
```

## What it generates

From Chelsea's OpenAPI spec (27 schemas, 15 operations):

| Language | Files | Features |
|----------|-------|----------|
| TypeScript | client.ts, models.ts, index.ts, package.json, tsconfig.json | fetch-based, typed request/response bodies |
| Rust | client.rs, models.rs, lib.rs, Cargo.toml | reqwest + serde, typed structs and enums |
| Python | client.py, models.py, \_\_init\_\_.py, pyproject.toml | httpx, dataclasses, async/await |
| Go | client.go, models.go, go.mod | net/http, typed structs with json tags |

All SDKs include:
- **Typed models** from `components/schemas` (structs, enums, nested refs)
- **Typed request/response bodies** from `$ref` resolution
- **Path parameter interpolation** (e.g. `vm_id` as function argument)
- **Bearer token authentication**
- **Generated README** with usage examples

## LLM Enhancement

With `--enhance`, each generated source file is post-processed through Claude for:
- Doc comment improvements
- Error handling polish
- Language-idiomatic adjustments

Enhancement is non-destructive — if the API call fails, the original generated code is used.

## Architecture

```
src/
  main.zig              CLI: generate, init, version
  parser/openapi.zig    OpenAPI 3.1 JSON parser (schemas, operations, $refs)
  config/config.zig     TOML config loader
  config/toml.zig       TOML parser
  generator/
    sdk.zig             Core generator (iterates targets, renders templates)
    template.zig        Mustache-like engine ({{var}}, {{#each}}, {{#if}})
  llm/enhancer.zig      Optional LLM post-processing via Anthropic API
templates/
  typescript/           6 templates (client, models, index, package.json, ...)
  rust/                 4 templates (client, models, lib, Cargo.toml)
  python/               5 templates (client, models, __init__, pyproject, ...)
  go/                   4 templates (client, models, go.mod, README)
  zig/                  3 templates (client, build.zig, README)
```

## Chelsea automation

Sterling is configured to auto-generate SDKs from [hdresearch/chelsea](https://github.com/hdresearch/chelsea)'s OpenAPI spec:

```bash
# Generate from Chelsea's spec
./zig-out/bin/sterling generate \
  --spec ../chelsea/openapi/openapi.json \
  --config sterling.toml

# Sync docs to vers-docs (Mintlify)
./sync-vers-docs.sh ../chelsea/openapi/openapi.json
```

The GitHub Actions workflow (`.github/workflows/sterling-automation.yml`) runs on:
- `repository_dispatch` from Chelsea when the spec changes
- Daily schedule (6am UTC)
- Manual `workflow_dispatch`

It generates SDKs and pushes to per-language repos, then syncs the spec to `hdresearch/vers-docs`.

## License

MIT
