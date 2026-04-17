# Sterling SDK Generator — Code Cannon Plan

ALL SYSTEMS AND AGENTS MUST use this github → https://github.com/hdresearch/sterling.git

Sterling is a Zig-based OpenAPI SDK generator that produces TypeScript, Rust, Python, and Go SDKs from an OpenAPI 3.1.0 spec. It currently generates working SDKs that compile, but lacks production-quality features compared to Stainless (the tool it replaces). The reference Stainless output is at https://github.com/hdresearch/vers-sdk-ts.git — clone it and study its patterns.

## Architecture

- `src/main.zig` — CLI entry point, parses args, calls generator
- `src/parser/openapi.zig` — Parses OpenAPI 3.1.0 JSON specs into Zig structs
- `src/config/config.zig` — Parses `sterling.toml` TOML config (project name, targets, LLM settings)
- `src/generator/sdk.zig` — Core generator: builds template contexts from parsed spec, renders templates per language
- `src/generator/template.zig` — Mustache-like template engine (supports `{{var}}`, `{{#if}}`, `{{#each}}`, `{{#unless}}`)
- `templates/{typescript,rust,python,go}/` — Language-specific templates that get rendered with context variables
- `chelsea/openapi/orchestrator.openapi.json` — The real OpenAPI spec (39 endpoints, 19 query params, oneOf unions)

## Build & Test

```bash
zig build -Doptimize=ReleaseFast
./zig-out/bin/sterling generate --spec chelsea/openapi/orchestrator.openapi.json --config sterling.toml
# Verify TypeScript
cd generated/typescript && npm install typescript --save-dev && npx tsc --noEmit
# Verify Rust
cd generated/rust && cargo check
# Verify Go
cd generated/go && go vet ./...
# Verify Python
cd generated/python && python3 -m py_compile src/client.py && python3 -m py_compile src/models.py
```

ALL four languages must compile/vet clean after every change. Run the full verify after every modification.

## What Needs to Be Built

For each of the below goals, create a VM and run code like the following:

```bash
while true; do
  pi -p "GOAL"
done
```

NOTE — pi is running on the VM itself. Include pi-vers extension so each infinite loop can provision further VMs or agents. Every VM should have the env vars `VERS_API_KEY`, `ANTHROPIC_API_KEY`, `GITHUB_API_KEY`.

---

### AGENT 1: Query Parameter Support (all 4 languages)

**Priority: CRITICAL — this is a functional bug causing API failures.**

The OpenAPI spec has 19 query parameters across 12 endpoints (e.g. `skip_wait_boot`, `count`, `keep_paused`, `wait_boot`, `offset`, `max_entries`, `stream`, `vm_id`). Sterling currently ignores them entirely — all parameters are treated as either path params or request body.

**What to do:**

1. In `src/parser/openapi.zig`, parse `parameters` arrays from operations. Each parameter has `"in": "query"`, `"name"`, `"schema"`, and `"required"`. Store these on the operation struct alongside the existing path/body data.

2. In `src/generator/sdk.zig`, when building operation contexts, create a `query_params` list with each param's name, type, whether it's required, and its language-specific type mapping. Add template variables: `has_query_params`, `query_params`, `query_params_ts`, `query_params_rust`, `query_params_python`, `query_params_go`.

3. Update ALL language templates to accept query params as function arguments and append them to the URL as `?key=value&key2=value2`:

   - **TypeScript** (`templates/typescript/client.ts.template`): Add optional params object, build URLSearchParams, append to path
   - **Rust** (`templates/rust/client.rs.template`): Add Option<T> params, build query string with serde_urlencoded or manual append
   - **Python** (`templates/python/client.py.template`): Add `**kwargs` or typed params, pass as `params=` to httpx/requests
   - **Go** (`templates/go/client.go.template`): Add params, build `url.Values`, append to URL

4. Endpoints that have BOTH query params AND a request body (e.g. `POST /api/v1/vm/{vm_id}/commit` has `keep_paused` + `skip_wait_boot` as query, plus body) must handle both correctly — query params go on the URL, body goes in the POST payload.

**Reference:** Look at how Stainless handles `VmCommitParams` — it separates `keep_paused` (query) from `commit_id` (body) and sends them to different places.

**Verify:** After changes, all 4 SDKs must still compile. Manually inspect that `createNewRootVm` has a `wait_boot` query param and `commitVm` has both query params AND a body.

---

### AGENT 2: JSDoc / Doc Comments on Model Fields (all 4 languages)

**Priority: HIGH — affects developer experience.**

The OpenAPI spec has `description` fields on almost every schema property. Sterling currently emits bare interface fields with no documentation. Stainless emits JSDoc on every field.

**What Sterling currently generates (TypeScript):**
```typescript
export interface NewRootRequest {
  vm_config: VmCreateVmConfig;
}
```

**What it should generate:**
```typescript
export interface NewRootRequest {
  /** Struct representing configuration options common to all VMs */
  vm_config: VmCreateVmConfig;
}
```

**What to do:**

1. In `src/parser/openapi.zig`, ensure property descriptions are parsed and stored. Check that `Property.description` is being populated from the spec's `properties.*.description` field.

2. In `src/generator/sdk.zig`, when building model/property contexts, include the `description` field in each property context.

3. Update all 4 model templates:
   - **TypeScript** (`templates/typescript/models.ts.template`): Add `/** {{description}} */` above each field
   - **Rust** (`templates/rust/models.rs.template`): Add `/// {{description}}` above each field
   - **Python** (`templates/python/models.py.template`): Add field docstring or `#: {{description}}` comment
   - **Go** (`templates/go/models.go.template`): Add `// {{description}}` above each field

4. Also add doc comments on enum variants where the spec provides them.

**Reference:** Compare `VmExecRequest` in Stainless output vs Sterling output — every field in Stainless has a JSDoc comment.

---

### AGENT 3: Error Types and Response Handling (TypeScript focus, then all languages)

**Priority: HIGH — affects production usability.**

Sterling returns raw `Response` or casts `resp.json() as Promise<T>` with no error handling. Stainless has a full error hierarchy (`BadRequestError`, `AuthenticationError`, `NotFoundError`, `RateLimitError`, etc.) and an `APIPromise` wrapper.

**What to do:**

1. Create a new template file `templates/typescript/errors.ts.template` with an error class hierarchy:
   - `VersSDKError` (base)
   - `APIError` (has status, headers, body)
   - `BadRequestError` (400), `AuthenticationError` (401), `PermissionDeniedError` (403), `NotFoundError` (404), `ConflictError` (409), `UnprocessableEntityError` (422), `RateLimitError` (429), `InternalServerError` (5xx)
   - `APIConnectionError`, `APIConnectionTimeoutError`

2. Update `templates/typescript/client.ts.template`:
   - Import error classes
   - In the `request()` method, check `response.ok` — if not, parse the body and throw the appropriate error class based on status code
   - Return typed responses properly (not just `resp.json() as Promise<T>`)

3. Update `templates/typescript/index.ts.template` to re-export error types.

4. Register the new template in `src/generator/sdk.zig`'s `generateTypeScript()` function.

5. Then do the same for other languages:
   - **Rust**: Custom error enum with variants per status code, implement `std::error::Error`
   - **Python**: Exception classes inheriting from a base `VersSDKError`
   - **Go**: Error types with status code checking

**Reference:** Clone `https://github.com/hdresearch/vers-sdk-ts.git` and study `src/core/error.ts`. Match its patterns.

---

### AGENT 4: Resource-Based Organization (TypeScript first)

**Priority: MEDIUM — affects API ergonomics.**

Sterling puts all 39 methods on a single flat class. Stainless organizes them into resource groups: `client.vm.branch()`, `client.repositories.create()`, `client.commits.list()`.

**What to do:**

1. In `src/parser/openapi.zig` or `src/generator/sdk.zig`, group operations by their first path segment after `/api/v1/`:
   - `/api/v1/vm/*` and `/api/v1/vms` → `vm` resource
   - `/api/v1/repositories/*` → `repositories` resource
   - `/api/v1/commits/*` → `commits` resource
   - `/api/v1/commit_tags/*` → `commitTags` resource
   - `/api/v1/domains/*` → `domains` resource
   - `/api/v1/env_vars/*` → `envVars` resource
   - `/api/v1/public/*` → `publicRepositories` resource

2. Create a new template `templates/typescript/resource.ts.template` for individual resource files.

3. Update the TypeScript client template so the main class has resource accessors:
   ```typescript
   class VersSDKClient {
     readonly vm: VmResource;
     readonly repositories: RepositoriesResource;
     // ...
     constructor(options) {
       this.vm = new VmResource(this);
       // ...
     }
   }
   ```

4. Update `generateTypeScript()` in `sdk.zig` to render one file per resource group plus the main client.

5. Keep the flat client available too (or as an alternative) — don't break the existing API.

**Reference:** Study `src/resources/vm.ts`, `src/resources/repositories.ts` etc. in the Stainless SDK.

---

### AGENT 5: Retries, Timeouts, and Env Var Defaults (TypeScript first, then all)

**Priority: MEDIUM — affects production reliability.**

Sterling's HTTP client is a bare `fetch()` wrapper with no retries, no configurable timeout, and no env var reading.

**What to do:**

1. Update `templates/typescript/client.ts.template`:
   - Add `maxRetries` option (default 2) and `timeout` option (default 30000ms)
   - Implement retry logic with exponential backoff for 5xx errors and connection failures
   - Read `VERS_API_KEY` and `VERS_BASE_URL` from `process.env` as defaults
   - Use `AbortController` for timeout support

2. For **Rust**: Add retry logic in the reqwest client builder, read env vars with `std::env::var()`
3. For **Python**: Add retry with backoff, read env vars with `os.environ.get()`
4. For **Go**: Add retry logic in the HTTP client, read env vars with `os.Getenv()`

**Reference:** The Stainless client constructor reads `VERS_API_KEY`, `VERS_BASE_URL`, and `VERS_LOG` from env. It implements retry with exponential backoff and jitter.

---

### AGENT 6: oneOf / Union Type Support

**Priority: MEDIUM — one schema affected currently, but important for spec evolution.**

The spec has `FromCommitVmRequest` as a `oneOf` with three variants: `{ commit_id }`, `{ tag_name }`, `{ ref }`. Sterling currently generates this as a single flat interface, losing the union semantics.

**What to do:**

1. In `src/parser/openapi.zig`, detect `oneOf` in schema definitions. Store the variants.

2. In `src/generator/sdk.zig`, when a model has `oneOf`, produce a discriminated union:
   - **TypeScript**: `type FromCommitVmRequest = { commit_id: string } | { tag_name: string } | { ref: string }`
   - **Rust**: `enum FromCommitVmRequest { CommitId { commit_id: String }, TagName { tag_name: String }, Ref { ref_: String } }`
   - **Python**: `FromCommitVmRequest = Union[CommitIdVariant, TagNameVariant, RefVariant]`
   - **Go**: Interface-based or struct with optional fields

3. Update model templates to handle the `is_union` flag.

**Reference:** Stainless generates `VmFromCommitRequest = VmFromCommitRequest.CommitID | VmFromCommitRequest.TagName | VmFromCommitRequest.Ref` with namespaced interfaces.

---

### AGENT 7: Test Suite Generation

**Priority: MEDIUM — needed before publishing to registries.**

Sterling generates zero tests. Stainless generates test files for each resource.

**What to do:**

1. Create test templates for each language:
   - `templates/typescript/tests/client.test.ts.template` — test that each method constructs the right URL, method, and body
   - `templates/rust/tests/client_test.rs.template` — similar with `#[tokio::test]`
   - `templates/python/tests/test_client.py.template` — pytest-based
   - `templates/go/client_test.go.template` — standard Go testing

2. Tests should mock the HTTP layer and verify:
   - Correct URL construction (path params interpolated, query params appended)
   - Correct HTTP method
   - Correct request body serialization
   - Error handling for non-2xx responses

3. Register test templates in each language's generator function in `sdk.zig`.

4. Add a CI step that runs the tests after generation.

**Reference:** Study `tests/api-resources/vm.test.ts` in the Stainless SDK.

---

### AGENT 8: SSH Library (TypeScript only)

**Priority: LOW — nice to have, Stainless has it, but it's a hand-written library not generated from the spec.**

The Stainless SDK includes an SSH-over-TLS client for connecting to Vers VMs. This is not auto-generated from the OpenAPI spec — it's a hand-written library in `src/lib/ssh/`.

**What to do:**

1. Copy the SSH library pattern: after generating the SDK, include a `src/lib/ssh/` directory with:
   - `client.ts` — SSH-over-TLS using `ssh2` library
   - `errors.ts` — SSH-specific error types
   - `types.ts` — Connection options, execute results, shell sessions
   - `index.ts` — Re-exports

2. This can be a static template (not generated from the spec) that lives in `templates/typescript/lib/ssh/`.

3. Add `ssh2` as a dependency in `package.json.template`.

4. Add a convenience method on the main client: `client.ssh(vmId)` that fetches the SSH key and returns an SSHClient.

**Reference:** Clone `https://github.com/hdresearch/vers-sdk-ts.git` and copy `src/lib/ssh/` patterns exactly. The SSH client connects over TLS to port 443, then runs SSH protocol on top.

---

## Priority Order

1. **AGENT 1** (query params) — CRITICAL, functional bug
2. **AGENT 2** (doc comments) — HIGH
3. **AGENT 3** (error types) — HIGH
4. **AGENT 5** (retries/timeouts) — MEDIUM, do after error types
5. **AGENT 4** (resource organization) — MEDIUM
6. **AGENT 6** (oneOf unions) — MEDIUM
7. **AGENT 7** (tests) — MEDIUM
8. **AGENT 8** (SSH library) — LOW

Agents 1, 2, and 6 can run in parallel (they touch different parts: parser+templates, model templates, parser+model templates).
Agents 3 and 5 should run sequentially (5 depends on 3's error types).
Agent 4 can run independently but should wait for Agent 1 (query params affect method signatures).
Agent 7 should run after 1-5 are merged (tests need to verify the new features).
Agent 8 is independent of everything.
