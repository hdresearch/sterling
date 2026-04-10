# Chelsea Eagle Test Infrastructure

Documentation for the Eagle test suite helper packages in `lib/Chelsea1.0/`. Each `.eagle` source file has a corresponding `.eagle.md` reference documenting its procedures, parameters, and usage.

## Packages

| File | Description |
|------|-------------|
| [apis.eagle.md](./apis.eagle.md) | API client wrappers for public, internal, and proxy endpoints |
| [configuration.eagle.md](./configuration.eagle.md) | Configuration, environment variables, and `.env` file handling |
| [database.eagle.md](./database.eagle.md) | SQLite connection management, row operations, and binary I/O |
| [fileSystem.eagle.md](./fileSystem.eagle.md) | File packaging, manifest processing, token expansion, and deployment |
| [gpg.eagle.md](./gpg.eagle.md) | GPG signing and signature verification |
| [json.eagle.md](./json.eagle.md) | JSON processing via `jq` |
| [legacy.eagle.md](./legacy.eagle.md) | Legacy test helpers (daemon lifecycle, cluster management, CLI wrappers) |
| [meta.eagle.md](./meta.eagle.md) | Meta package that loads all Chelsea packages |
| [network.eagle.md](./network.eagle.md) | Network utilities (HTTP calls, response validation) |
| [shell.eagle.md](./shell.eagle.md) | Shell command building and execution |
| [ssh.eagle.md](./ssh.eagle.md) | SSH command execution on VMs |
| [temporaryDirectory.eagle.md](./temporaryDirectory.eagle.md) | Temporary directory creation, verification, and cleanup |
| [test.eagle.md](./test.eagle.md) | Core test harness (VM lifecycle, constraint checking, test setup/teardown) |
| [value.eagle.md](./value.eagle.md) | Value validation (GUIDs, ports, IPs, file names, timestamps, etc.) |
| [web.eagle.md](./web.eagle.md) | Web/HTTP request helpers |
| [wireGuard.eagle.md](./wireGuard.eagle.md) | WireGuard interface and peer management |

## Conventions

All doc files follow a consistent structure:

- **Signature** — proc name and parameter defaults
- **Params** — argument descriptions and expectations
- **Returns** — return value documentation
- **Raises** — error conditions
- **Notes** — side effects and dependencies
- **Dependencies & Environment** — external tools and other Chelsea helpers required

## Source Files

The corresponding source files live in `lib/Chelsea1.0/*.eagle`. The package index is managed by `pkgIndex.eagle`.
