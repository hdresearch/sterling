# Vers TypeScript API Library

This library provides convenient access to the [Vers](https://hdr.is) REST API from server-side TypeScript or JavaScript.

It is generated with [Sterling](https://github.com/hdresearch/sterling).

## Installation

```sh
npm install vers-sdk
```

## Usage

```ts
import { VersSdkClient } from "vers-sdk";

const client = new VersSdkClient({
  apiKey: process.env["VERS_API_KEY"], // This is the default and can be omitted
});

// Create a new root VM
const vm = await client.createNewRootVm({ vm_config: {} });
console.log(vm.vm_id);

// Branch from an existing VM
const branches = await client.branchVm("vm-id", {}, { count: 3 });
console.log(branches.vms);

// Resource-based access
const vms = await client.vm.listVms();
```

## Configuration

```ts
const client = new VersSdkClient({
  apiKey: "your-api-key",       // or set VERS_API_KEY env var
  baseUrl: "https://api.vers.sh", // or set VERS_BASE_URL env var
  maxRetries: 2,                // default: 2
  timeout: 30000,               // default: 30s
  logLevel: "warn",             // or set VERS_LOG env var
  fetch: customFetch,           // custom fetch implementation
});
```

## Handling errors

When the API returns a non-success status code, a subclass of `APIError` is thrown:

```ts
import { BadRequestError, NotFoundError } from "vers-sdk";

try {
  await client.deleteVm("nonexistent-id");
} catch (err) {
  if (err instanceof NotFoundError) {
    console.log(err.status);  // 404
    console.log(err.message);
  }
}
```

| Status Code | Error Type                 |
| ----------- | -------------------------- |
| 400         | `BadRequestError`          |
| 401         | `AuthenticationError`      |
| 403         | `PermissionDeniedError`    |
| 404         | `NotFoundError`            |
| 409         | `ConflictError`            |
| 422         | `UnprocessableEntityError` |
| 429         | `RateLimitError`           |
| ≥500        | `InternalServerError`      |
| N/A         | `APIConnectionError`       |

## Retries

Certain errors are automatically retried 2 times by default with exponential backoff.
Connection errors, 408 Request Timeout, 409 Conflict, 429 Rate Limit, and ≥500 Internal
errors are all retried. The client respects `Retry-After` headers.

```ts
const client = new VersSdkClient({ maxRetries: 0 }); // disable retries

// Or per-request:
await client.createNewRootVm({ vm_config: {} }, undefined, { timeout: 5000 });
```

## Per-request options

Every method accepts an optional `RequestOptions` parameter as the last argument:

```ts
await client.listVms({ headers: { "X-Custom": "value" }, timeout: 5000 });
```

## SSH

The SDK includes an SSH-over-TLS client for connecting to Vers VMs:

```ts
import { SSHClient } from "vers-sdk/lib/ssh";
```

## Requirements

- TypeScript ≥ 4.9
- Node.js 20+, Deno, Bun, or Cloudflare Workers

## License

MIT
