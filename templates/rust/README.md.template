# Vers Rust API Library

This library provides convenient access to the [Vers](https://hdr.is) REST API from Rust.

It is generated with [Sterling](https://github.com/hdresearch/sterling).

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
vers-sdk = { git = "https://github.com/hdresearch/rust-sdk" }
```

## Usage

```rust
use vers_sdk::client::VersSdkClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = VersSdkClient::builder()
        .api_key("your-api-key")  // or set VERS_API_KEY env var
        .build();

    // List all VMs
    let response = client.list_vms(None).await?;
    println!("{:?}", response.status());

    // Create a new root VM
    let vm = client.create_new_root_vm(
        &vers_sdk::models::NewRootRequest { vm_config: None },
        None,
        None,
    ).await?;

    Ok(())
}
```

## Configuration

```rust
let client = VersSdkClient::builder()
    .api_key("your-api-key")
    .base_url("https://api.vers.sh")
    .max_retries(2)
    .timeout(std::time::Duration::from_secs(30))
    .log_level(vers_sdk::client::LogLevel::Warn)
    .build();
```

## Error handling

The client returns `reqwest::Error` for HTTP failures. Check the response status for API-level errors.

## Retries

Requests are automatically retried up to 2 times on 5xx errors and connection failures,
with exponential backoff and jitter. The client respects `Retry-After` headers (capped at 60s).

## Requirements

- Rust 1.70+
- tokio runtime

## License

MIT
