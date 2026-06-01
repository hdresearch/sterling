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
    """Fix old Stainless SDK import patterns and known typos."""
    replacements = [
        ("from 'vers';", "from 'vers-sdk';"),
        ('from "vers";', 'from "vers-sdk";'),
        ("import Vers, { withSSH }", "import { VersSdkClient }"),
        ("fs_size_vm_mib", "fs_size_mib"),
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


def _build_change_identifiers(changes: dict, spec: dict) -> set[str]:
    """Extract all identifiers (endpoint paths, operationIds, schema names, field names)
    that were touched by the API changes. Used to filter which docs files need LLM updates."""
    ids = set()

    for ep in changes["added_endpoints"] + changes["removed_endpoints"] + changes["changed_endpoints"]:
        ids.add(ep["path"])
        ids.add(ep.get("operationId", ""))
        # Extract path segments as identifiers (e.g. "vm", "commits", "ssh_key")
        for segment in ep["path"].strip("/").split("/"):
            if segment and not segment.startswith("{") and segment not in ("api", "v1"):
                ids.add(segment)

    old_schemas = spec.get("_old_schemas", {})  # stashed by caller
    new_schemas = spec.get("components", {}).get("schemas", {})

    for name in changes["added_schemas"] + changes["removed_schemas"] + changes["changed_schemas"]:
        ids.add(name)
        # Include changed field names so we can match files referencing them
        old_props = set(old_schemas.get(name, {}).get("properties", {}).keys())
        new_props = set(new_schemas.get(name, {}).get("properties", {}).keys())
        ids.update(old_props ^ new_props)  # symmetric difference = added + removed fields

    ids.discard("")
    return ids


def find_affected_files(docs_dir: Path, changes: dict, spec: dict) -> list[tuple[Path, set[str]]]:
    """Find .mdx files that reference identifiers touched by the API changes.
    Returns (filepath, set_of_matched_identifiers) tuples — only files that
    actually reference something that changed."""
    change_ids = _build_change_identifiers(changes, spec)
    if not change_ids:
        return []

    # Build a regex from the change identifiers (escape them, match as words)
    escaped = [re.escape(id_) for id_ in change_ids if len(id_) > 2]
    if not escaped:
        return []
    pattern = re.compile(r'\b(' + '|'.join(escaped) + r')\b')

    # Skip files we regenerate deterministically
    skip = {"api-reference/introduction.mdx", "sdks.mdx"}
    results = []

    for mdx in sorted(docs_dir.rglob("*.mdx")):
        rel = str(mdx.relative_to(docs_dir))
        if rel in skip:
            continue
        content = mdx.read_text()
        matches = set(pattern.findall(content))
        if matches:
            results.append((mdx, matches))

    return results


def _build_surgical_context(matches: set[str], changes: dict, spec: dict) -> str:
    """Build a minimal context string containing only the spec details relevant
    to what this specific file references. Much smaller than dumping the full spec."""
    sections = []

    # 1. Always include the change summary (it's compact)
    if changes["summary"]:
        sections.append("What changed in the API:\n" + "\n".join(f"  • {s}" for s in changes["summary"]))

    new_schemas = spec.get("components", {}).get("schemas", {})
    new_paths = spec.get("paths", {})

    # 2. Include details only for schemas this file references
    referenced_schemas = set()
    for name in list(changes.get("added_schemas", [])) + list(changes.get("removed_schemas", [])) + list(changes.get("changed_schemas", [])):
        if name in matches:
            referenced_schemas.add(name)
    # Also check if any match is a field inside a changed schema
    for name in changes.get("changed_schemas", []):
        schema = new_schemas.get(name, {})
        props = set(schema.get("properties", {}).keys())
        if matches & props:
            referenced_schemas.add(name)

    if referenced_schemas:
        schema_lines = []
        for name in sorted(referenced_schemas):
            schema = new_schemas.get(name, {})
            props = schema.get("properties", {})
            fields = []
            for fname, fdef in props.items():
                ftype = fdef.get("type", fdef.get("$ref", "object").split("/")[-1])
                fields.append(f"    {fname}: {ftype}")
            schema_lines.append(f"  {name}:\n" + "\n".join(fields))
        sections.append("Affected schemas (current fields):\n" + "\n".join(schema_lines))

    # 3. Include details only for endpoints this file references
    referenced_endpoints = []
    all_changed_eps = changes["added_endpoints"] + changes["removed_endpoints"] + changes["changed_endpoints"]
    for ep in all_changed_eps:
        ep_ids = {ep["path"], ep.get("operationId", "")}
        for segment in ep["path"].strip("/").split("/"):
            if segment and not segment.startswith("{") and segment not in ("api", "v1"):
                ep_ids.add(segment)
        if matches & ep_ids:
            # Include the full endpoint spec for context
            path_spec = new_paths.get(ep["path"], {})
            method_lower = ep["method"].lower()
            op_spec = path_spec.get(method_lower, {})
            summary = op_spec.get("summary", "")
            params = [p.get("name", "") for p in op_spec.get("parameters", []) if p.get("in") == "query"]
            req_ref = ""
            rb = op_spec.get("requestBody", {})
            if rb:
                content = rb.get("content", {}).get("application/json", {})
                schema = content.get("schema", {})
                req_ref = schema.get("$ref", "").split("/")[-1] if "$ref" in schema else ""
            detail = f"  {ep['method']} {ep['path']}"
            if summary:
                detail += f" — {summary}"
            if params:
                detail += f"\n    query params: {', '.join(params)}"
            if req_ref:
                detail += f"\n    request body: {req_ref}"
            referenced_endpoints.append(detail)

    if referenced_endpoints:
        sections.append("Affected endpoints (current spec):\n" + "\n".join(referenced_endpoints))

    return "\n\n".join(sections)


def update_file_with_llm(filepath: Path, docs_dir: Path, spec: dict, changes: dict, matches: set[str]) -> bool:
    """Use Claude to update a single docs file based on spec changes. Returns True if changed.
    Only sends context relevant to what this file actually references (surgical)."""
    content = filepath.read_text()
    rel = str(filepath.relative_to(docs_dir))

    context = _build_surgical_context(matches, changes, spec)

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

{context}

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
            # Stash old schemas on spec so the surgical context builder can diff fields
            new_spec["_old_schemas"] = old_spec.get("components", {}).get("schemas", {})

            affected = find_affected_files(docs_dir, changes, new_spec)
            if affected:
                print(f"\n── LLM updates ({len(affected)} files reference changed API surface) ──")
                updated_count = 0
                for filepath, matches in affected:
                    rel = str(filepath.relative_to(docs_dir))
                    print(f"  {rel}: matched [{', '.join(sorted(matches)[:5])}{'…' if len(matches) > 5 else ''}]")
                    if update_file_with_llm(filepath, docs_dir, new_spec, changes, matches):
                        updated_count += 1
                print(f"\n  {updated_count}/{len(affected)} files updated by LLM")
            else:
                print("\n  No docs files reference the changed API surface — skipping LLM")

            # Clean up stashed data
            new_spec.pop("_old_schemas", None)
    elif use_llm:
        print("\n── LLM updates skipped (no meaningful API changes) ──")

    print("\n✅ Done")


if __name__ == "__main__":
    main()
