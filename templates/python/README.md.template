# Vers Python API Library

This library provides convenient access to the [Vers](https://hdr.is) REST API from Python.

It is generated with [Sterling](https://github.com/hdresearch/sterling).

## Installation

```sh
pip install vers-sdk
```

## Usage

```python
import asyncio
from vers_sdk import VersSdkClient

async def main():
    client = VersSdkClient(
        api_key="your-api-key",  # or set VERS_API_KEY env var
    )

    # List all VMs
    response = await client.list_vms()
    print(response.json())

    # Create a new root VM
    vm = await client.create_new_root_vm(body={"vm_config": {}})
    print(vm.json())

    await client.close()

asyncio.run(main())
```

## Configuration

```python
client = VersSdkClient(
    api_key="your-api-key",       # or set VERS_API_KEY env var
    base_url="https://api.vers.sh", # or set VERS_BASE_URL env var
    max_retries=2,
    timeout=30.0,
    http_client=custom_httpx_client,  # optional custom httpx.AsyncClient
)
```

## Error handling

```python
from vers_sdk.errors import APIError, NotFoundError, RateLimitError

try:
    await client.delete_vm("nonexistent-id")
except NotFoundError as e:
    print(f"Not found: {e.status} {e.message}")
except APIError as e:
    print(f"API error: {e.status}")
```

## Retries

Requests are automatically retried up to 2 times on 5xx errors and connection failures,
with exponential backoff and jitter. The client respects `Retry-After` headers (capped at 60s).

## Async context manager

```python
async with VersSdkClient(api_key="...") as client:
    vms = await client.list_vms()
```

## Requirements

- Python 3.10+
- httpx

## License

MIT
