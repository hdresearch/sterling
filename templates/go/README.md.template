# Vers Go API Library

This library provides convenient access to the [Vers](https://hdr.is) REST API from Go.

It is generated with [Sterling](https://github.com/hdresearch/sterling).

## Installation

```sh
go get github.com/hdresearch/go-sdk
```

## Usage

```go
package main

import (
	"fmt"
	vers "github.com/hdresearch/go-sdk"
)

func main() {
	client := vers.NewClient(
		"https://api.vers.sh",
		"your-api-key", // or set VERS_API_KEY env var
	)

	// List all VMs
	resp, err := client.ListVms()
	if err != nil {
		panic(err)
	}
	fmt.Println(resp.StatusCode)

	// Branch a VM
	resp, err = client.BranchVm("vm-id", nil, &vers.BranchVmParams{
		Count: intPtr(3),
	})
}

func intPtr(i int64) *int64 { return &i }
```

## Configuration

```go
client := vers.NewClient("https://api.vers.sh", "your-api-key",
	vers.WithHTTPClient(&http.Client{Timeout: 60 * time.Second}),
)
client.MaxRetries = 3
client.LogLevel = vers.LogInfo
```

## Error handling

```go
resp, err := client.DeleteVm("nonexistent-id", nil)
if err != nil {
	fmt.Println("connection error:", err)
}
if resp.StatusCode == 404 {
	fmt.Println("VM not found")
}
```

## Retries

Requests are automatically retried up to 2 times on 5xx errors and connection failures,
with exponential backoff and jitter. The client respects `Retry-After` headers (capped at 60s).

## Requirements

- Go 1.21+

## License

MIT
