# Chelsea Updater Daemon

A background service that automatically keeps the Chelsea binary up-to-date by monitoring GitHub releases.

## Features

- **Automatic Updates**: Monitors GitHub releases and downloads new versions automatically
- **Smart Updating**: Only downloads when checksums differ, avoiding unnecessary transfers
- **Checksum Verification**: Uses SHA256 hashes to verify file integrity
- **AWS Integration**: Securely retrieves GitHub tokens from AWS Secrets Manager
- **Structured Logging**: Uses `tracing` for comprehensive logging with configurable levels
- **Error Resilience**: Continues running even if individual update checks fail

## Installation

```bash
cargo build --release
```

## Configuration

### Environment Variables

Configure logging with the `RUST_LOG` environment variable:

```bash
# Info level logging (recommended for production)
export RUST_LOG=info

# Debug level logging (for troubleshooting)
export RUST_LOG=debug

# Specific module logging
export RUST_LOG=updater_daemon=debug,reqwest=info

# Trace everything (very verbose)
export RUST_LOG=trace
```

### AWS Setup

The daemon requires AWS CLI to be configured with access to AWS Secrets Manager:

1. Install AWS CLI: `aws configure`
2. Ensure your AWS credentials have access to the secret `github-token-chelsea`
3. The secret should contain a GitHub personal access token with repository access

## Usage

### Running the Daemon

```bash
# With default logging
./target/release/updater-daemon

# With debug logging
RUST_LOG=debug ./target/release/updater-daemon

# With JSON structured logging
RUST_LOG=info ./target/release/updater-daemon 2>&1 | jq
```

### Log Output Examples

```
2023-12-06T10:30:00.123456Z  INFO updater_daemon: Chelsea Updater Daemon Starting...
2023-12-06T10:30:00.234567Z  INFO updater_daemon: Fetching current binary checksum from GitHub...
2023-12-06T10:30:01.345678Z  INFO updater_daemon: Remote binary info: 1234567 bytes, SHA256: abc123...
2023-12-06T10:30:01.456789Z  INFO updater_daemon: No local binary found at /bin/chelsea_daemon
2023-12-06T10:30:01.567890Z  INFO updater_daemon: Binary needs to be downloaded/updated...
2023-12-06T10:30:01.678901Z  INFO updater_daemon: Downloading binary from GitHub (Asset ID: 98765)...
2023-12-06T10:30:01.789012Z  INFO updater_daemon: Binary size: 1234567 bytes
2023-12-06T10:30:01.890123Z  INFO updater_daemon: Binary created at: 2023-12-06T09:15:30.123Z
2023-12-06T10:30:05.789012Z  INFO updater_daemon: Made binary executable
2023-12-06T10:30:05.890123Z  INFO updater_daemon: Checksum verification passed!
2023-12-06T10:30:05.901234Z  INFO updater_daemon: Binary successfully updated!
2023-12-06T10:30:05.912345Z  INFO updater_daemon: Downloaded from: https://api.github.com/repos/hdresearch/chelsea/releases/assets/98765
2023-12-06T10:30:05.923456Z  INFO updater_daemon: Download completed at: 2023-12-06T10:30:05.912Z
2023-12-06T10:30:05.934567Z  INFO updater_daemon: Asset ID: 98765, Size: 1234567 bytes
2023-12-06T10:30:05.945678Z  INFO updater_daemon: Starting update checker (checking every 5 minutes)...
```

## Error Handling

The daemon uses structured error types for better error handling and debugging:

- `GitHubApi`: GitHub API related errors
- `Network`: Network connectivity issues
- `FileSystem`: File I/O errors
- `ChecksumMismatch`: Binary integrity verification failures
- `BinaryNotFound`: Missing binary assets in releases
- `Authentication`: AWS/GitHub authentication failures
- `Download`: Binary download failures

## Security Considerations

- GitHub tokens are retrieved securely from AWS Secrets Manager
- All downloads are verified with SHA256 checksums
- The daemon requires appropriate file system permissions to write to `/bin/`
- Uses HTTPS for all network communications

## Development

### Running Tests

```bash
cargo test
```

### Code Structure

- `main.rs`: Core daemon logic and orchestration
- `binary_checker.rs`: Checksum computation and GitHub API interactions
- `gh.rs`: GitHub authentication and binary downloads
- `types.rs`: Data structures and type definitions

### Structured Data Types

The daemon uses well-defined structs for type safety and better data handling:

- **`PrecomputedChecksum`**: Contains filename, size, and SHA256 hash information
- **`BinaryInfo`**: Tracks download URL, hash, and download timestamp
- **`BinaryMetadata`**: Stores asset ID, size, creation/update times, path, and checksum
- **`GitHubRelease`**: Structured representation of GitHub release data
- **`GitHubAsset`**: Individual asset information from GitHub releases

These types provide:

- **Type Safety**: Compile-time guarantees about data structure
- **Better Error Messages**: Clear field names instead of raw JSON access
- **Enhanced Logging**: Structured information for better observability
- **Future Extensions**: Easy to add new fields and functionality
