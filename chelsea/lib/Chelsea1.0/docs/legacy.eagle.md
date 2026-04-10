# Chelsea Legacy Test Helpers

Procedures in the `::Chelsea` namespace providing test suite helper functions for daemon lifecycle management, API command execution, file operations, and test utilities. These are core procedures used throughout the Chelsea test harness.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value (often `""` or `formatExecResults` dict).
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects, globals, or dependencies.

---

## Small Utilities

### `getVarOrNull varName`

* **Purpose:** In `-cleanup` blocks, fetch a possibly-unset variable; if missing, return `null`.
* **Params:**
  * `varName` — name of the variable to check in caller's scope.
* **Returns:** Variable value or `null` (`[debug null]`).
* **Example:**

  ```tcl
  -cleanup {
    set h [::Chelsea::getVarOrNull handle]
    if {$h ne $::null} { object dispose $h }
  }
  ```

### `getTestApiKey`

* **Purpose:** Read the first API key from the service database.
* **Flow:** `openDatabase` → `SELECT id FROM api_key WHERE rowId = 1;` → `closeDatabase`.
* **Returns:** API key GUID string.
* **Raises:** if database is missing or no key exists.

---

## Command Execution

### `execMakeCommand withRoot args`

* **Purpose:** Run a `make` command using the project Makefile.
* **Params:**
  * `withRoot` — if `true`, execute via `sudo`.
  * `args` — arguments to pass to make.
* **Returns:** `formatExecResults` dict, or raw result if background (`&`).
* **Notes:**
  * Uses global `makeFile` for Makefile path.
  * Runs with `-s -f $makeFile --` options.
  * Changes to Makefile's directory during execution.
* **Example:**

  ```tcl
  set r [::Chelsea::execMakeCommand false build-release]
  ```

### `execAdminCommand args`

* **Purpose:** Run the admin CLI tool.
* **Params:**
  * `args` — arguments to pass to the admin command.
* **Returns:** `formatExecResults` dict, or raw result if background (`&`).
* **Notes:** Uses global `commandsFile` for the CLI path.
* **Example:**

  ```tcl
  set r [::Chelsea::execAdminCommand stop -y]
  ```

### `execApiCommand args`

* **Purpose:** Run the API CLI tool with automatic API key injection.
* **Params:**
  * `args` — arguments to pass to the API command.
* **Returns:** `formatExecResults` dict, or raw result if background (`&`).
* **Notes:**
  * Uses global `apiFile` for the CLI path.
  * Automatically appends `--key [getTestApiKey]` if args are provided.
* **Example:**

  ```tcl
  set r [::Chelsea::execApiCommand create-cluster]
  ```

---

## File Operations

### `maybeCopyFiles sourceDirectory targetDirectory patterns`

* **Purpose:** Copy matching files (including dotfiles) if not already present at target.
* **Params:**
  * `sourceDirectory` — source directory path.
  * `targetDirectory` — destination directory path.
  * `patterns` — list of basename patterns (no slashes allowed).
* **Raises:**
  * if source or target directory doesn't exist.
  * if any pattern contains a `/` character.
* **Notes:** Only copies files that don't already exist in target.
* **Example:**

  ```tcl
  ::Chelsea::maybeCopyFiles /src /dest [list *.dll .env]
  ```

### `maybeWriteDotEnvFile targetDirectory overwrite environment channel verbose quiet`

* **Purpose:** Generate and write a `.env` configuration file with test settings.
* **Params:**
  * `targetDirectory` — directory where `.env` will be created.
  * `overwrite` — if `true`, overwrite existing variables.
  * `environment` — if `true`, also set `::env` variables.
  * `channel` — output channel for log messages.
  * `verbose` — if `true`, use `tputs`; otherwise use `tlog`.
  * `quiet` — if `true`, suppress all output.
* **Raises:** if target directory doesn't exist.
* **Variables Written:**
  * `NODE_ID` — random hex identifier.
  * `DATA_DIR` — data directory path.
  * `DB_SCHEMA_PATH` — database schema file path.
  * `DB_CLEANUP_PATH` — database cleanup file path.
  * `DB_PATH` — database file path.
  * `getDefaultTestLog` — current test log path.
  * `NoCleanupEnabled` / `NO_FETCH_FS_MOVE` / `NO_FETCH_FS_CLEANUP` — if cleanup disabled.
  * `NoStopEnabled` — if stop disabled.
  * `DISK_VM_MIB` / `DISK_CACHE_MIB` / `DISK_LOG_MIB` / `DISK_SYSTEM_MIB` / `DISK_SSHKEYS_MIB` — disk quotas.
  * `MEMORY_MIB_MARGIN` / `CPU_CORES_MARGIN` — resource margins.
  * `ADMIN_API_KEY` — admin API key.

---

## JSON Helpers

### `testExecApiCommand command jpaths varName args`

* **Purpose:** Run an API command, parse JSON output, and optionally extract a value via JSON paths.
* **Params:**
  * `command` — the API command to run.
  * `jpaths` — optional list of JSON path selectors.
  * `varName` — optional variable name to upvar the full JSON.
  * `args` — additional arguments.
* **Returns:** Full JSON string, or extracted value if `jpaths` provided.
* **Raises:** if JSON is invalid after extraction attempts.
* **Example:**

  ```tcl
  set status [::Chelsea::testExecApiCommand "status --json" {data status}]
  ```

### `isSuccessJson channel json default quiet`

* **Purpose:** Heuristic checker for successful JSON responses.
* **Params:**
  * `channel` — output channel for messages.
  * `json` — JSON string to check.
  * `default` — value to return if indeterminate.
  * `quiet` — if `true`, suppress error messages.
* **Returns:** `true` if success, `false` otherwise, or `default`.
* **Notes:** Success if: error is absent/empty AND (data present OR status == "Completed").

### `extractJson value`

* **Purpose:** Strip known progress/logging prefixes from mixed output to extract JSON.
* **Params:**
  * `value` — output string potentially containing JSON.
* **Returns:** The extracted JSON portion.
* **Raises:** if no JSON can be extracted.
* **Patterns matched:**
  * `Fetching rootfs ...`
  * `Fetching kernel ...`
  * `Stopping daemon Auto-confirming cleanup (due to -y flag) ...`

### `verifyFetchSuccess channel name args`

* **Purpose:** Run a command, extract JSON, and verify it indicates success.
* **Params:**
  * `channel` — output channel.
  * `name` — optional field name to extract from result.
  * `args` — command to execute via `uplevel 1`.
* **Raises:** on invalid or unsuccessful JSON.

---

## Cluster/VM Operations

### `verifyMachineAndClusterIds jsonVarName idsVarName machineName machineOperator clusterJPaths clusterOperator`

* **Purpose:** Cross-validate machine and cluster IDs from JSON response.
* **Params:**
  * `jsonVarName` — variable name containing JSON.
  * `idsVarName` — variable name containing ids array.
  * `machineName` — name key for machine ID comparison.
  * `machineOperator` — comparison operator (default: `eq`).
  * `clusterJPaths` — optional JSON paths for cluster ID.
  * `clusterOperator` — cluster comparison operator (default: `eq`).
* **Raises:** on missing/invalid IDs or comparison mismatches.

### `createCluster varName`

* **Purpose:** Create a cluster via the API and return IDs.
* **Params:**
  * `varName` — optional variable name to upvar the ids array.
* **Returns/Upvars:**
  * `ids(machine)` — first VM GUID.
  * `ids(cluster)` — cluster ID.
  * `ids(address)` — first VM IP address.
* **Raises:** on invalid JSON or IDs.

### `cleanupViaApi channel type id`

* **Purpose:** Best-effort deletion of a cluster or VM via the API.
* **Params:**
  * `channel` — output channel.
  * `type` — `cluster` or `vm`.
  * `id` — entity identifier.
* **Returns:** `true` if success observed, `false` otherwise.
* **Raises:** if type or id is invalid.
* **Notes:** Tries normal deletion first, then with `--force` or `--recursive`.

---

## Daemon Lifecycle

### `setupDaemon channel`

* **Purpose:** Start daemon with API key and fetch required artifacts.
* **Flow:** `startDaemon createApiKey true` → `fetch-kernel` → `fetch-fs`.
* **Notes:** Verifies both fetch operations succeed.

### `cleanupDaemon id channel`

* **Purpose:** Stop daemon and clean up resources.
* **Params:**
  * `id` — optional cluster ID to delete first.
  * `channel` — output channel.
* **Flow:** `cleanupViaApi` (if id) → `stopDaemon` → `info previouspid true`.

### `startDaemon channel initialize createApiKey`

* **Purpose:** Build, copy, initialize, and start the Chelsea daemon.
* **Params:**
  * `channel` — output channel.
  * `initialize` — if `true`, run `init-db`.
  * `createApiKey` — if `true`, run `create-api-key`.
* **Flow:**
  1. Build executable if not present.
  2. Create target directory structure.
  3. Copy binaries, libraries, tools, and SQLite.
  4. Write `.env` configuration.
  5. Change owner to `root`.
  6. Initialize database (if requested).
  7. Create API key (if requested).
  8. Start daemon process (optionally under `strace`).
* **Returns:** Empty string.
* **Raises:** on build failure or initialization errors.
* **Notes:** Stores PID in `::eagle_debugger(daemonPid)`.

### `stopDaemon channel cleanup`

* **Purpose:** Stop the daemon process and optionally run cleanup.
* **Params:**
  * `channel` — output channel.
  * `cleanup` — if `true`, run cleanup command.
* **Returns:** Empty string.
* **Raises:** on stop or cleanup failure.
* **Notes:**
  * Skips if `isStopEnabled` returns false.
  * Waits for process exit using `waitForProcesses`.

---

## Process Checking

### `makeSureChelseaIsNotRunning channel quiet`

* **Purpose:** Assert the Chelsea daemon is not currently running.
* **Raises:** with error and log message if any Chelsea processes are found.

### `checkForFirecrackerBuild channel`

* **Purpose:** Add `firecrackerBuild` constraint if building for Firecracker.
* **Notes:** Checks `PARENT_MAKE_TARGET` environment variable.
* **Returns:** Prints `yes` or `no` to channel.

---

## Dependencies & Globals

* **Globals:**
  * `makeFile` — path to Makefile.
  * `commandsFile` — path to admin CLI.
  * `apiFile` — path to API CLI.
  * `exeFile` — path to daemon executable.
  * `::test_timeout` — timeout in milliseconds.
  * `::eagle_debugger(daemonPid)` — daemon process ID.
* **Other Chelsea helpers:** database, shell, configuration, SSH helpers.
* **Eagle packages:** `Eagle.Test`.

---

*Package:* `Chelsea.Legacy 1.0` · *Namespace:* `::Chelsea`
