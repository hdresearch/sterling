#!/usr/bin/env python3
"""
Sterling docs updater — propagates OpenAPI spec changes into vers-docs.

1. Diffs old spec (from vers-docs main) against new spec (from chelsea)
2. Deterministically regenerates api-reference/introduction.mdx
3. Uses Claude to update code samples in tutorials/examples that reference the API
4. Patches docs.json nav and fixes stale SDK imports

Usage:
    python3 scripts/update-docs.py <vers-docs-dir> <new-spec-path> <version> [--llm]

Without --llm, only deterministic updates are applied.
With --llm and ANTHROPIC_API_KEY set, tutorials/examples get LLM-updated code samples.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

# ─── Spec diffing ──────────────────────────────────────────────────────────

def load_spec(path: str) -> dict:
    with open(path) as f:
        return json.load(f)


def diff_specs(old: dict, new: dict) -> dict:
    """Produce a structured diff of two OpenAPI specs."""
    changes = {
        "added_endpoints": [],
        "removed_endpoints": [],
        "changed_endpoints": [],
        "added_schemas": [],
        "removed_schemas": [],
        "changed_schemas": [],
        "summary": [],
    }

    old_paths = old.get("paths", {})
    new_paths = new.get("paths", {})

    # Endpoints
    old_ops = {(p, m): op for p, methods in old_paths.items()
               for m, op in methods.items() if m in ("get","post","put","delete","patch")}
    new_ops = {(p, m): op for p, methods in new_paths.items()
               for m, op in methods.items() if m in ("get","post","put","delete","patch")}

    for key in sorted(set(new_ops) - set(old_ops)):
        op = new_ops[key]
        changes["added_endpoints"].append({
            "method": key[1].upper(), "path": key[0],
            "operationId": op.get("operationId", "?"),
            "summary": op.get("summary", ""),
        })
        changes["summary"].append(f"Added {key[1].upper()} {key[0]} ({op.get('operationId','')})")

    for key in sorted(set(old_ops) - set(new_ops)):
        op = old_ops[key]
        changes["removed_endpoints"].append({
            "method": key[1].upper(), "path": key[0],
            "operationId": op.get("operationId", "?"),
        })
        changes["summary"].append(f"Removed {key[1].upper()} {key[0]} ({op.get('operationId','')})")

    for key in sorted(set(old_ops) & set(new_ops)):
        if json.dumps(old_ops[key], sort_keys=True) != json.dumps(new_ops[key], sort_keys=True):
            changes["changed_endpoints"].append({
                "method": key[1].upper(), "path": key[0],
                "operationId": new_ops[key].get("operationId", "?"),
            })
            changes["summary"].append(f"Changed {key[1].upper()} {key[0]} ({new_ops[key].get('operationId','')})")

    # Schemas
    old_schemas = old.get("components", {}).get("schemas", {})
    new_schemas = new.get("components", {}).get("schemas", {})

    for name in sorted(set(new_schemas) - set(old_schemas)):
        changes["added_schemas"].append(name)
        changes["summary"].append(f"Added schema: {name}")

    for name in sorted(set(old_schemas) - set(new_schemas)):
        changes["removed_schemas"].append(name)
        changes["summary"].append(f"Removed schema: {name}")

    for name in sorted(set(old_schemas) & set(new_schemas)):
        if json.dumps(old_schemas[name], sort_keys=True) != json.dumps(new_schemas[name], sort_keys=True):
            changes["changed_schemas"].append(name)
            # Get field-level detail
            old_props = set(old_schemas[name].get("properties", {}).keys())
            new_props = set(new_schemas[name].get("properties", {}).keys())
            added = new_props - old_props
            removed = old_props - new_props
            if added:
                changes["summary"].append(f"Schema {name}: added fields {', '.join(sorted(added))}")
            if removed:
                changes["summary"].append(f"Schema {name}: removed fields {', '.join(sorted(removed))}")
            if not added and not removed:
                changes["summary"].append(f"Schema {name}: field types/metadata changed")

    return changes


def is_meaningful_change(changes: dict) -> bool:
    """Return True if there are API-affecting changes (not just metadata)."""
    return bool(
        changes["added_endpoints"] or changes["removed_endpoints"] or
        changes["changed_endpoints"] or changes["added_schemas"] or
        changes["removed_schemas"] or changes["changed_schemas"]
    )


# ─── Deterministic updates ─────────────────────────────────────────────────

def generate_introduction(spec: dict, version: str) -> str:
    """Regenerate api-reference/introduction.mdx from the spec."""
    paths = spec.get("paths", {})
    schemas = spec.get("components", {}).get("schemas", {})

    # Build endpoint table
    endpoint_rows = []
    for path in sorted(paths.keys()):
        for method in ("get", "post", "put", "delete", "patch"):
            if method in paths[path]:
                op = paths[path][method]
                desc = op.get("summary", op.get("description", "").split(".")[0])
                endpoint_rows.append(f"| `{method.upper()}` | `{path}` | {desc} |")

    endpoints_table = "\n".join(endpoint_rows)
    num_endpoints = len(endpoint_rows)

    # Build schemas list
    schema_names = sorted(schemas.keys())

    # Extract VmCreateVmConfig fields for request body example
    vm_config = schemas.get("VmCreateVmConfig", {}).get("properties", {})
    vm_config_fields = ", ".join(f'"{k}"' for k in sorted(vm_config.keys())[:5])

    return f"""---
title: "API Introduction"
description: "Overview of the Vers API — {num_endpoints} endpoints for programmatic VM management"
---

# API Introduction

The Vers API provides programmatic access to VM management, commits, repositories, domains, environment variables, and tagging. While most users interact with Vers through the CLI or [SDKs](/sdks), the API enables direct integration.

## Base URL

```
https://api.vers.sh/api/v1
```

## Authentication

All API endpoints require a Bearer token:

```bash
curl -H "Authorization: Bearer $VERS_API_KEY" \\
  https://api.vers.sh/api/v1/vms
```

Get your API key from [vers.sh/billing](https://vers.sh/billing).

## Endpoints

{num_endpoints} operations across VMs, commits, repositories, domains, environment variables, and tags:

| Method | Endpoint | Description |
|--------|----------|-------------|
{endpoints_table}

## Models

{len(schema_names)} types defined in the schema:

{', '.join(f'`{s}`' for s in schema_names)}

See the interactive API playground below for full request/response schemas.

## Quick Examples

### Create a VM

```bash
curl -X POST https://api.vers.sh/api/v1/vm/new_root?wait_boot=true \\
  -H "Authorization: Bearer $VERS_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{{"vm_config": {{"mem_size_mib": 512, "vcpu_count": 1, "fs_size_mib": 512}}}}'
```

### List VMs

```bash
curl https://api.vers.sh/api/v1/vms \\
  -H "Authorization: Bearer $VERS_API_KEY"
```

### Get SSH Credentials

```bash
curl https://api.vers.sh/api/v1/vm/{{vm_id}}/ssh_key \\
  -H "Authorization: Bearer $VERS_API_KEY"
```

### Branch a VM

```bash
curl -X POST https://api.vers.sh/api/v1/vm/{{vm_id}}/branch \\
  -H "Authorization: Bearer $VERS_API_KEY"
```

## SDKs

Official SDKs are available for 9 languages — see the [SDKs page](/sdks) for install instructions and quickstart code.

## See Also

- [SDKs](/sdks) — Install and quickstart for TypeScript, Python, Go, Rust, Ruby, C#, Java, Kotlin, PHP
- [VM Access Guide](/vm-access) — SSH connections and SDK usage
- [CLI Reference](/cli-reference/init) — Command-line interface
"""


def update_sdk_page(version: str, spec: dict) -> str:
    """Regenerate sdks.mdx with current operation count and version."""
    paths = spec.get("paths", {})
    num_ops = sum(
        1 for methods in paths.values()
        for m in methods if m in ("get","post","put","delete","patch")
    )

    # Read current sdks.mdx if it exists, otherwise generate from scratch
    return f"""---
title: "SDKs"
description: "Official Vers SDKs for 9 languages — auto-generated from the OpenAPI spec by Sterling"
---

# SDKs

Vers publishes official SDKs for **9 languages**, all auto-generated from the OpenAPI spec and published to package registries on every API change.

## Installation

<Tabs>
  <Tab title="TypeScript">
    ```bash
    npm install vers-sdk
    ```
    ```typescript
    import {{ VersSdkClient }} from "vers-sdk";

    const client = new VersSdkClient({{ apiKey: process.env.VERS_API_KEY }});
    const vms = await client.listVms();
    ```
    [GitHub](https://github.com/hdresearch/ts-sdk) · [npm](https://www.npmjs.com/package/vers-sdk)
  </Tab>
  <Tab title="Python">
    ```bash
    pip install vers-sdk
    ```
    ```python
    from vers_sdk import VersSdkClient

    client = VersSdkClient(api_key=os.environ["VERS_API_KEY"])
    vms = client.list_vms()
    ```
    [GitHub](https://github.com/hdresearch/python-sdk) · [PyPI](https://pypi.org/project/vers-sdk/)
  </Tab>
  <Tab title="Go">
    ```bash
    go get github.com/hdresearch/go-sdk
    ```
    ```go
    import vers "github.com/hdresearch/go-sdk"

    client := vers.NewVersSdkClient(os.Getenv("VERS_API_KEY"))
    vms, err := client.ListVms(nil)
    ```
    [GitHub](https://github.com/hdresearch/go-sdk)
  </Tab>
  <Tab title="Rust">
    ```bash
    cargo add vers-sdk
    ```
    ```rust
    use vers_sdk::VersSdkClient;

    let client = VersSdkClient::new(std::env::var("VERS_API_KEY")?);
    let vms = client.list_vms(None).await?;
    ```
    [GitHub](https://github.com/hdresearch/rust-sdk) · [crates.io](https://crates.io/crates/vers-sdk)
  </Tab>
  <Tab title="Ruby">
    ```bash
    gem install vers-sdk
    ```
    ```ruby
    require "vers_sdk"

    client = VersSdk::Client.new(api_key: ENV["VERS_API_KEY"])
    vms = client.list_vms
    ```
    [GitHub](https://github.com/hdresearch/ruby-sdk) · [RubyGems](https://rubygems.org/gems/vers-sdk)
  </Tab>
  <Tab title="C#">
    ```bash
    dotnet add package vers-sdk
    ```
    ```csharp
    using VersSdk;

    var client = new VersClient(Environment.GetEnvironmentVariable("VERS_API_KEY"));
    var vms = await client.ListVmsAsync();
    ```
    [GitHub](https://github.com/hdresearch/csharp-sdk) · [NuGet](https://www.nuget.org/packages/vers-sdk)
  </Tab>
  <Tab title="Java">
    ```xml
    <!-- Maven — coming soon to Maven Central -->
    <dependency>
      <groupId>com.vers</groupId>
      <artifactId>vers-sdk</artifactId>
      <version>{version}</version>
    </dependency>
    ```
    ```java
    import com.vers.sdk.VersClient;

    VersClient client = new VersClient(System.getenv("VERS_API_KEY"));
    List<VM> vms = client.listVms(null);
    ```
    [GitHub](https://github.com/hdresearch/java-sdk)
  </Tab>
  <Tab title="Kotlin">
    ```kotlin
    // Gradle
    implementation("com.vers:vers-sdk:{version}")
    ```
    ```kotlin
    import com.vers.sdk.VersClient

    val client = VersClient(System.getenv("VERS_API_KEY"))
    val vms = client.listVms()
    ```
    [GitHub](https://github.com/hdresearch/kotlin-sdk)
  </Tab>
  <Tab title="PHP">
    ```bash
    composer require vers/sdk
    ```
    ```php
    use VersSdk\\VersClient;

    $client = new VersClient(getenv('VERS_API_KEY'));
    $vms = $client->listVms();
    ```
    [GitHub](https://github.com/hdresearch/php-sdk)
  </Tab>
</Tabs>

## Features

All SDKs include:

- **Full API coverage** — {num_ops} operations across VMs, commits, repositories, domains, environment variables, and tags
- **Typed models** — Request/response types generated from the OpenAPI schema
- **Error handling** — Typed error responses with status codes
- **Automatic retries** — Configurable retry with exponential backoff and `Retry-After` support
- **Request options** — Per-request timeout, headers, and idempotency key overrides
- **Query parameters** — Typed parameter interfaces for filtering and pagination
- **Platform detection** — `User-Agent` header with SDK version, language runtime, and OS info

## Authentication

All SDKs accept an API key via constructor or environment variable:

```bash
export VERS_API_KEY="your-api-key"
```

Get your API key from [vers.sh/billing](https://vers.sh/billing).

## Source

SDKs are auto-generated by [Sterling](https://github.com/hdresearch/sterling) from the [Chelsea OpenAPI spec](https://github.com/hdresearch/chelsea) and published on every API change.
"""


def patch_docs_json(docs_dir: Path):
    """Add SDKs tab to docs.json if not present."""
    docs_json = docs_dir / "docs.json"
    with open(docs_json) as f:
        docs = json.load(f)

    tabs = docs["navigation"]["tabs"]
    if any(t.get("tab") == "SDKs" for t in tabs):
        print("  docs.json: SDKs tab already present")
        return

    api_idx = next((i for i, t in enumerate(tabs) if t.get("tab") == "API"), len(tabs))
    tabs.insert(api_idx, {
        "tab": "SDKs",
        "groups": [{"group": "SDKs", "pages": ["sdks"]}]
    })

    with open(docs_json, "w") as f:
        json.dump(docs, f, indent=2)
        f.write("\n")
    print("  docs.json: Added SDKs tab")


def fix_stale_imports(docs_dir: Path):
    """Fix old Stainless SDK import patterns."""
    replacements = [
        ("from 'vers';", "from 'vers-sdk';"),
        ('from "vers";', 'from "vers-sdk";'),
        ("import Vers, { withSSH }", "import { VersSdkClient }"),
    ]
    for mdx in docs_dir.rglob("*.mdx"):
        content = mdx.read_text()
        changed = False
        for old, new in replacements:
            if old in content:
                content = content.replace(old, new)
                changed = True
        if changed:
            mdx.write_text(content)
            print(f"  {mdx.relative_to(docs_dir)}: Fixed stale SDK imports")


# ─── LLM-powered updates ──────────────────────────────────────────────────

def call_claude(system: str, user: str) -> str:
    """Call the Anthropic Messages API."""
    import urllib.request

    api_key = os.environ.get("ANTHROPIC_API_KEY", "")
    if not api_key:
        raise RuntimeError("ANTHROPIC_API_KEY not set")

    body = json.dumps({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 8192,
        "system": system,
        "messages": [{"role": "user", "content": user}],
    }).encode()

    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages",
        data=body,
        headers={
            "Content-Type": "application/json",
            "x-api-key": api_key,
            "anthropic-version": "2023-06-01",
        },
    )

    with urllib.request.urlopen(req, timeout=120) as resp:
        data = json.loads(resp.read())
        return data["content"][0]["text"]


def find_api_coupled_files(docs_dir: Path) -> list[Path]:
    """Find .mdx files that contain API endpoint references, SDK code, or model names."""
    patterns = [
        r"/api/v1/", r"api\.vers\.sh", r"new_root", r"from_commit", r"ssh_key",
        r"mem_size_mib", r"vcpu_count", r"fs_size_mib", r"vm_config",
        r"VersClient\b", r"VersSdkClient\b", r"vers_sdk", r"vers-sdk",
        r"from 'vers", r'from "vers',
        r"versApi\(", r'fetch\(`\$\{BASE\}',
    ]
    combined = re.compile("|".join(patterns))

    # Skip files we regenerate deterministically
    skip = {"api-reference/introduction.mdx", "sdks.mdx"}
    results = []

    for mdx in sorted(docs_dir.rglob("*.mdx")):
        rel = str(mdx.relative_to(docs_dir))
        if rel in skip:
            continue
        content = mdx.read_text()
        if combined.search(content):
            results.append(mdx)

    return results


def update_file_with_llm(filepath: Path, docs_dir: Path, spec: dict, changes: dict) -> bool:
    """Use Claude to update a single docs file based on spec changes. Returns True if changed."""
    content = filepath.read_text()
    rel = str(filepath.relative_to(docs_dir))

    # Build a compact spec summary for context
    endpoints_summary = []
    for path, methods in sorted(spec["paths"].items()):
        for m in ("get","post","put","delete","patch"):
            if m in methods:
                op = methods[m]
                endpoints_summary.append(f"{m.upper()} {path} — {op.get('operationId','')}")

    schemas_summary = []
    for name, schema in sorted(spec.get("components",{}).get("schemas",{}).items()):
        props = list(schema.get("properties", {}).keys())
        schemas_summary.append(f"{name}: {', '.join(props[:8])}" + ("..." if len(props) > 8 else ""))

    system = """You update documentation files to match the current Vers API.

Rules:
- Only modify code samples, API references, endpoint URLs, request/response JSON, and model field names
- Do NOT change prose style, section structure, MDX frontmatter, or tutorial flow
- Do NOT add or remove sections — only update what's already there
- Use the vers-sdk package name (not 'vers')
- Use VersSdkClient (TypeScript), VersSdkClient (Python), VersClient (other languages)
- Keep API base URL as https://api.vers.sh/api/v1
- If a code sample references an endpoint/model that was removed, update it to use the closest replacement
- If a code sample references an endpoint/model that was renamed, update the reference
- If nothing needs changing, return the file unchanged
- Return ONLY the complete file content, no commentary or markdown fences around it"""

    user = f"""Here is the documentation file `{rel}`:

<file>
{content}
</file>

Here is what changed in the API:
{json.dumps(changes['summary'], indent=2) if changes['summary'] else 'No structural changes, but models/endpoints may have updated fields.'}

Here is the current API (all endpoints):
{chr(10).join(endpoints_summary)}

Here are the current models (with their fields):
{chr(10).join(schemas_summary)}

Update the file so all code samples, API endpoint references, request/response JSON examples, and model field names match the current API. Return the complete updated file."""

    try:
        updated = call_claude(system, user)
        # Strip any markdown code fences Claude might add despite instructions
        updated = re.sub(r'^```\w*\n', '', updated)
        updated = re.sub(r'\n```$', '', updated.rstrip())

        if updated.strip() != content.strip():
            filepath.write_text(updated)
            print(f"  {rel}: Updated by LLM")
            return True
        else:
            print(f"  {rel}: No changes needed")
            return False
    except Exception as e:
        print(f"  {rel}: LLM update failed ({e}), skipping")
        return False


# ─── Main ──────────────────────────────────────────────────────────────────

def main():
    if len(sys.argv) < 4:
        print(f"Usage: {sys.argv[0]} <vers-docs-dir> <new-spec-path> <version> [--llm]")
        sys.exit(1)

    docs_dir = Path(sys.argv[1])
    new_spec_path = sys.argv[2]
    version = sys.argv[3]
    use_llm = "--llm" in sys.argv

    old_spec_path = docs_dir / "api-reference" / "openapi.json"

    print(f"Sterling docs updater v{version}")
    print(f"  Docs dir:  {docs_dir}")
    print(f"  New spec:  {new_spec_path}")
    print(f"  LLM mode:  {'enabled' if use_llm else 'disabled'}")

    # Load specs
    new_spec = load_spec(new_spec_path)
    old_spec = load_spec(str(old_spec_path)) if old_spec_path.exists() else {"paths": {}, "components": {"schemas": {}}}

    # Diff
    changes = diff_specs(old_spec, new_spec)
    if changes["summary"]:
        print(f"\nAPI changes detected ({len(changes['summary'])}):")
        for s in changes["summary"]:
            print(f"  • {s}")
    else:
        print("\nNo API changes detected")

    # 1. Copy spec
    print("\n── Deterministic updates ──")
    import shutil
    shutil.copy2(new_spec_path, str(docs_dir / "api-reference" / "openapi.json"))
    print("  api-reference/openapi.json: Updated")

    # 2. Regenerate introduction.mdx
    intro = generate_introduction(new_spec, version)
    (docs_dir / "api-reference" / "introduction.mdx").write_text(intro)
    print("  api-reference/introduction.mdx: Regenerated")

    # 3. Regenerate sdks.mdx
    sdks = update_sdk_page(version, new_spec)
    (docs_dir / "sdks.mdx").write_text(sdks)
    print("  sdks.mdx: Regenerated")

    # 4. Patch docs.json
    patch_docs_json(docs_dir)

    # 5. Fix stale imports
    fix_stale_imports(docs_dir)

    # 6. LLM-powered updates for tutorials/examples
    if use_llm and (is_meaningful_change(changes) or not old_spec_path.exists()):
        api_key = os.environ.get("ANTHROPIC_API_KEY", "")
        if not api_key:
            print("\n⚠️  ANTHROPIC_API_KEY not set, skipping LLM updates")
        else:
            coupled_files = find_api_coupled_files(docs_dir)
            if coupled_files:
                print(f"\n── LLM updates ({len(coupled_files)} files) ──")
                updated_count = 0
                for f in coupled_files:
                    if update_file_with_llm(f, docs_dir, new_spec, changes):
                        updated_count += 1
                print(f"\n  {updated_count}/{len(coupled_files)} files updated by LLM")
            else:
                print("\n  No API-coupled docs files found")
    elif use_llm:
        print("\n── LLM updates skipped (no meaningful API changes) ──")

    print("\n✅ Done")


if __name__ == "__main__":
    main()
