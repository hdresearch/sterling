# Sterling vs Stainless — Feature Comparison

Sterling is an open-source OpenAPI SDK generator written in Zig. It was built to replace [Stainless](https://www.stainless.com/) for generating the Vers API client libraries. This document compares the two generators as of April 2026, using the `hdresearch/chelsea` Orchestrator Control Plane API spec (OpenAPI 3.1.0, 35 paths, 48 operations).

## At a glance

|                            | Stainless             | Sterling                    |
| -------------------------- | --------------------- | --------------------------- |
| **Output languages**       | TypeScript only       | TypeScript, Rust, Python, Go |
| **API operations covered** | 43                    | 48                          |
| **Model types generated**  | 78                    | 57                          |
| **TypeScript output**      | 5,713 lines / 44 files | 3,408 lines / 21 files     |
| **Total output (all langs)** | 5,713 lines         | 12,837 lines / 35 files    |
| **Test lines**             | 2,752 (TS)            | 2,717 (TS + Rust + Python + Go) |
| **Generator language**     | Proprietary (SaaS)    | Zig (open source)           |
| **Spec source**            | `openapi.stainless.yaml` (14 paths) | `orchestrator.openapi.json` (35 paths) |

Sterling covers more API surface because it generates from the full orchestrator spec (35 paths / 48 operations) while the Stainless-generated SDK was built from an older, smaller spec (14 paths / 43 operations including exec/stream endpoints no longer in the public API).

## Feature matrix

### Core SDK features

| Feature                              | Stainless | Sterling |
| ------------------------------------ | --------- | -------- |
| Typed request/response models        | ✅        | ✅       |
| Path parameter interpolation         | ✅        | ✅       |
| Query parameter support              | ✅        | ✅       |
| Typed `*Params` interfaces           | ✅        | ✅       |
| Bearer token authentication          | ✅        | ✅       |
| Environment variable defaults        | ✅        | ✅       |
| Doc comments on model fields         | ✅        | ✅       |
| Enum types                           | ✅        | ✅       |
| `oneOf` / discriminated union types  | ✅        | ✅       |
| Nested types (namespaces)            | ✅        | ✅       |

### Client infrastructure

| Feature                              | Stainless | Sterling |
| ------------------------------------ | --------- | -------- |
| Automatic retries with backoff       | ✅        | ✅       |
| `Retry-After` header parsing         | ✅        | ✅       |
| Configurable timeout                 | ✅        | ✅       |
| `APIPromise` / `.withResponse()`     | ✅        | ✅       |
| Per-request `RequestOptions`         | ✅        | ✅       |
| Typed error hierarchy (400–5xx)      | ✅        | ✅       |
| Idempotency keys (`X-Idempotency-Key`) | ✅     | ✅       |
| Custom `fetch` / HTTP client         | ✅        | ✅       |
| Header merging (null = remove)       | ✅        | ✅       |
| Platform detection / `User-Agent`    | ✅        | ✅       |
| Configurable logging (`VERS_LOG`)    | ✅        | ✅       |

### SDK organization

| Feature                              | Stainless | Sterling |
| ------------------------------------ | --------- | -------- |
| Resource-based method grouping       | ✅        | ✅       |
| Flat client access (all methods)     | ❌        | ✅       |
| Cross-platform shims (Deno/Bun/edge) | ✅        | ✅       |
| SSH-over-TLS library                 | ✅        | ✅       |
| Generated test suites                | ✅        | ✅       |

### Stainless-only features

These are present in the Stainless SDK but not in Sterling. None affect API call functionality.

| Feature                              | Notes |
| ------------------------------------ | ----- |
| File upload support                  | The Vers API has no upload endpoints — this is dead code in Stainless. |
| `fetchOptions` passthrough           | Sterling supports custom `fetch` function instead. |
| Custom logger injection              | Sterling logs to console with configurable level; no pluggable logger interface. |
| `APIPromise.asResponse()`            | Sterling has `APIPromise.withResponse()` but not the non-consuming variant. |
| Undocumented endpoint helpers        | `client.get()`, `client.post()` for raw HTTP — not generated from spec. |

### Sterling-only features

| Feature                              | Notes |
| ------------------------------------ | ----- |
| **4 output languages**               | TypeScript, Rust, Python, and Go from a single spec. Stainless only generates TypeScript. |
| **Flat + resource access**           | Methods available both as `client.branchVm()` and `client.vm.branchVm()`. |
| **Full API coverage**                | 48 operations covering repositories, domains, env vars, commit tags, public repos — endpoints not in the Stainless spec. |
| **Open-source generator**            | Zig source, MIT license, self-hostable. |

## Output comparison

### TypeScript

```
Stainless:  44 files,  5,713 lines
Sterling:   21 files,  3,408 lines  (40% smaller)
```

Sterling produces leaner output because it consolidates models into a single file and avoids Stainless's internal utility layer (~1,500 lines of header helpers, upload support, base64, path utilities, etc.).

### Rust (Sterling only)

```
5 files,  5,577 lines
```

reqwest + serde + tokio. Typed models, error handling, retries, logging, idempotency keys, header merging.

### Python (Sterling only)

```
5 files,  1,765 lines
```

httpx + dataclasses. Async client, typed models, retries, logging.

### Go (Sterling only)

```
4 files,  2,087 lines
```

net/http + encoding/json. Typed structs with json tags, retries, logging, functional options.

## Error hierarchies

Both generators produce equivalent typed error classes:

| Status | Stainless class            | Sterling class             |
| ------ | -------------------------- | -------------------------- |
| 400    | `BadRequestError`          | `BadRequestError`          |
| 401    | `AuthenticationError`      | `AuthenticationError`      |
| 403    | `PermissionDeniedError`    | `PermissionDeniedError`    |
| 404    | `NotFoundError`            | `NotFoundError`            |
| 409    | `ConflictError`            | `ConflictError`            |
| 422    | `UnprocessableEntityError` | `UnprocessableEntityError` |
| 429    | `RateLimitError`           | `RateLimitError`           |
| ≥500   | `InternalServerError`      | `InternalServerError`      |
| N/A    | `APIConnectionError`       | `APIConnectionError`       |
| N/A    | `APIUserAbortError`        | `APIConnectionTimeoutError`|

## Retry behavior

Both implement identical retry logic:

- **Default retries:** 2
- **Retryable statuses:** 408, 409, 429, ≥500
- **Backoff:** Exponential (500ms × 2^n) with 25% jitter
- **Retry-After:** Respected when present, capped at 60 seconds
- **Idempotency:** `X-Idempotency-Key` UUID header on POST/PUT/PATCH/DELETE

## Resource organization

Both group methods by API resource:

```typescript
// Stainless
client.vm.branch(vmId, body, params)
client.commits.list()
client.commitTags.create(body)

// Sterling
client.vm.branchVm(vmId, body, params)
client.commits.listCommits()
client.commitTags.createTag(body)

// Sterling also allows flat access:
client.branchVm(vmId, body, params)
```

## How this was built

Sterling's feature set was built in three waves using parallel VM agents ("code cannon"):

- **Wave 1** (5 agents): Query params, doc comments, error types, retries, union types
- **Wave 2** (6 agents): Resources, tests, SSH library, request options, params types, logging
- **Wave 3** (7 agents): RequestOptions threading, retry-after, idempotency, nested types, custom fetch, header merge, shims

Each wave branched VMs from a golden snapshot, ran autonomous coding agents in parallel, then merged results with manual conflict resolution. 18 agents total, all producing code that compiles clean across all 4 target languages.
