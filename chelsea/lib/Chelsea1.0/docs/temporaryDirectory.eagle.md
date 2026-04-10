# Chelsea Temporary Directory Helpers

Procedures in the `::Chelsea` namespace for safe temporary directory management. Provides creation, verification, and cleanup of temporary directories used during testing.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## Directory Initialization

### `maybeUseTemporaryDirectory permissions overwrite`

* **Purpose:** Initialize temporary directory variables and create necessary subdirectories.
* **Params:**
  * `permissions` — if `true`, set directory permissions to `ugo+rw` for non-root access.
  * `overwrite` — if `true`, overwrite existing variable values.
* **Side Effects:**
  * Sets `chelsea_data_directory` to `<tempdir>/var/lib/chelsea`.
  * Sets `chelsea_database_path` to `<datadir>/db/chelsea.db`.
  * Creates directories and empty database file if they don't exist.
  * Uses `sudo chmod -R ugo+rw` when `permissions` is true.
* **Notes:** Does nothing if `verifyTemporaryDirectory` returns empty.
* **Example:**

  ```tcl
  ::Chelsea::maybeCreateTemporaryDirectory
  ::Chelsea::maybeUseTemporaryDirectory true false
  ```

---

## Safety Validation

### `isSafeTemporaryDirectory directory`

* **Purpose:** Check if a directory path is safe to use/delete as a temporary directory.
* **Params:**
  * `directory` — path to validate.
* **Returns:** `true` if safe, `false` otherwise.
* **Validation Rules:**
  * Must be non-empty string.
  * Must exist.
  * Must be a directory.
  * Must be writable.
  * Must match one of these patterns:
    * `/run/user/<uid>/tmp/etd_<pid>` — user-specific temp
    * `/tmp/etd_<pid>` — system temp
* **Example:**

  ```tcl
  if {[::Chelsea::isSafeTemporaryDirectory /tmp/etd_12345]} {
    # Safe to use
  }
  ```

---

## Directory Lifecycle

### `maybeCreateTemporaryDirectory`

* **Purpose:** Create a temporary directory for test operations if one doesn't exist.
* **Side Effects:**
  * Creates directory at `<temppath>/tmp/etd_<pid>` or `<temppath>/etd_<pid>`.
  * Stores path in `temporaryDirectory` namespace variable.
* **Notes:**
  * Uses `file temppath` to determine base temp location.
  * Appends `/tmp` if the temp path doesn't already end in `tmp`.
  * Directory name includes current process ID for uniqueness.
* **Example:**

  ```tcl
  ::Chelsea::maybeCreateTemporaryDirectory
  set dir [::Chelsea::verifyTemporaryDirectory]
  puts "Using temp dir: $dir"
  ```

### `maybeDeleteTemporaryDirectory`

* **Purpose:** Delete the temporary directory if it's safe to do so.
* **Side Effects:**
  * Deletes the directory via `sudo rm -rf`.
  * Attempts `rmdir` as final cleanup.
  * Unsets `temporaryDirectory` variable.
* **Conditions for Deletion:**
  * `isCleanupEnabled` must return true.
  * `::no(deleteTemporaryDirectory)` must not be set.
  * `isSafeTemporaryDirectory` must return true.
* **Notes:** Fail-safe design prevents deletion of directories outside safe paths.
* **Example:**

  ```tcl
  ::Chelsea::maybeDeleteTemporaryDirectory
  ```

### `verifyTemporaryDirectory strict`

* **Purpose:** Verify the temporary directory exists and is usable.
* **Params:**
  * `strict` — if `true`, raise errors; if `false`, return empty string on failure.
* **Returns:** The temporary directory path, or empty string (non-strict mode).
* **Raises (strict mode):**
  * if `temporaryDirectory` variable is not set.
  * if directory doesn't exist.
  * if path is not a directory.
  * if path fails safety check.
* **Example:**

  ```tcl
  # Strict - raises on error
  set dir [::Chelsea::verifyTemporaryDirectory]

  # Non-strict - returns "" on error
  set dir [::Chelsea::verifyTemporaryDirectory false]
  if {$dir eq ""} {
    puts "No temp directory available"
  }
  ```

---

## Typical Workflow

**Standard test setup:**

```tcl
# Create and initialize temp directory
::Chelsea::maybeCreateTemporaryDirectory
::Chelsea::maybeUseTemporaryDirectory

# Get the path
set tempDir [::Chelsea::verifyTemporaryDirectory]
puts "Working in: $tempDir"

# ... run tests ...

# Cleanup
::Chelsea::maybeDeleteTemporaryDirectory
```

**Conditional cleanup:**

```tcl
::Chelsea::maybeCreateTemporaryDirectory

try {
  set dir [::Chelsea::verifyTemporaryDirectory]
  # ... test operations ...
} finally {
  # Only deletes if cleanup is enabled and path is safe
  ::Chelsea::maybeDeleteTemporaryDirectory
}
```

---

## Safe Path Patterns

The following path patterns are considered safe for deletion:

| Pattern | Description |
|---------|-------------|
| `/run/user/<uid>/tmp/etd_<pid>` | User-specific XDG runtime temp |
| `/tmp/etd_<pid>` | System-wide temp |

Where:
* `<uid>` is a numeric user ID.
* `<pid>` is a numeric process ID.

Any path not matching these patterns will **not** be deleted, even if cleanup is enabled.

---

## Dependencies & Environment

* **Namespace variables:**
  * `temporaryDirectory` — current temp directory path.
  * `chelsea_data_directory` — data directory path.
  * `chelsea_database_path` — database file path.
* **Other Chelsea helpers:** `buildExecCommand`, `evalExecCommand`, `isCleanupEnabled`.
* **External tools:** `chmod`, `rm`, `rmdir` (via sudo).

---

*Package:* `Chelsea.TemporaryDirectory 1.0` · *Namespace:* `::Chelsea`
