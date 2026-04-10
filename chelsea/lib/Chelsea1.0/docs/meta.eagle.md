# Chelsea Meta Package

The meta-package loader in the `::Chelsea` namespace that initializes and loads all Chelsea sub-packages. This is the main entry point for using the Chelsea test utilities.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Notes** adds side-effects or dependencies.

---

## Package Initialization

When `Chelsea.Meta` is loaded, it performs the following actions:

1. **Requires Eagle packages:**
   * `Eagle`
   * `Eagle.Execute.Pipes` (from `Extra1.0`)

2. **Sets package directory:**
   * Initializes `chelsea_package_directory` to the directory containing the meta script.

3. **Loads all sub-packages:**
   * `Chelsea.Apis` — API client wrappers
   * `Chelsea.Configuration` — Configuration and environment handling
   * `Chelsea.Database` — SQLite database helpers
   * `Chelsea.FileSystem` — File deployment and signing
   * `Chelsea.Gpg` — GPG signing and verification
   * `Chelsea.Json` — JSON processing via jq
   * `Chelsea.Network` — Network utilities (IP generation)
   * `Chelsea.Shell` — Shell command execution
   * `Chelsea.Ssh` — SSH key and command helpers
   * `Chelsea.TemporaryDirectory` — Temporary directory management
   * `Chelsea.Test` — Test harness helpers
   * `Chelsea.Value` — Validation and value utilities
   * `Chelsea.Web` — HTTP/cURL wrappers
   * `Chelsea.WireGuard` — WireGuard configuration

4. **Exports and imports:**
   * Exports all `::Chelsea::*` procedures.
   * Imports them into the global namespace.

5. **Initializes API mappings:**
   * Calls `initializeApiMappings` to load API endpoint definitions.

---

## Usage

```tcl
# Load the complete Chelsea package
package require Chelsea.Meta

# All procedures are now available in global namespace
createCluster ids
stopDaemon
```

---

## Sub-Package Loading

The following packages are explicitly loaded (in order):

| Package | Description |
|---------|-------------|
| `Chelsea.Apis` | Public, internal, and proxy API wrappers |
| `Chelsea.Configuration` | Environment and .env file handling |
| `Chelsea.Database` | SQLite connection and query helpers |
| `Chelsea.FileSystem` | Deployment database and file operations |
| `Chelsea.Gpg` | GPG signing and verification |
| `Chelsea.Json` | jq-based JSON processing |
| `Chelsea.Network` | Random IP address generation |
| `Chelsea.Shell` | Command execution and formatting |
| `Chelsea.Ssh` | SSH key writing and command execution |
| `Chelsea.TemporaryDirectory` | Safe temp directory management |
| `Chelsea.Test` | Test harness integration and hooks |
| `Chelsea.Value` | Input validation utilities |
| `Chelsea.Web` | cURL-based HTTP requests |
| `Chelsea.WireGuard` | WireGuard configuration helpers |

**Note:** `Chelsea.Legacy` is commented out and not loaded by default.

---

## Namespace Exports

After loading, all `::Chelsea::*` procedures are available in:
* The `::Chelsea` namespace (qualified: `::Chelsea::procName`)
* The global namespace (unqualified: `procName`)

---

## Dependencies

* **Eagle version:** Requires Eagle with `Eagle.Execute.Pipes` support.
* **File structure:** All `.eagle` files must be in the same directory as `meta.eagle`.

---

*Package:* `Chelsea.Meta 1.0` · *Namespace:* `::Chelsea`
