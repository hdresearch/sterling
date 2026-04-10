# Chelsea Test Harness Helpers

Procedures in the `::Chelsea` namespace (plus test hooks in `::`) for test suite integration, API endpoint mapping, JSON template processing, system environment queries, and test statistics collection.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects, globals, or dependencies.

---

## Design Philosophy

The package is guided by several principles that recur throughout the implementation:

* **Namespace isolation.** All helper procedures live in the `::Chelsea` namespace so that evaluating the package in plain Tcl does not pollute the global namespace. Test hooks are the sole exception — they are declared in `::` because the test harness expects them there.
* **Data-driven API mappings.** Endpoint definitions are loaded from a TSV file (`mappings.tsv`) rather than hard-coded, allowing endpoints to be added, removed, or changed without modifying procedural code.
* **Recursive template expansion.** JSON bodies are constructed from `.jsont` template files that may reference other templates or caller-scoped variables through `%name%` tokens, enabling composition without string concatenation.
* **Defensive validation.** Every public entry point validates its inputs (type enums, identifier formats, URI syntax, boolean flags) and raises an error with context before performing any work.
* **Fail-safe resource cleanup.** External commands that create temporary files (SSH keys, etc.) use `try`/`finally` to guarantee cleanup, guarded by `isCleanupEnabled` so that post-failure forensics can suppress deletion.
* **Opt-out statistics collection.** System environment queries run by default before and after every test. Individual queries can be suppressed via `::no(query,<name>)` variables rather than requiring opt-in, so new metrics are automatically captured unless explicitly disabled.
* **PID state preservation.** Every procedure that shells out saves and restores `[info previouspid]` so that the caller's view of the last-exec PID is undisturbed.

---

## Coding Conventions

### `nproc` vs `proc`

Procedures declared with `nproc` use **named parameters** — callers pass arguments as `key value` pairs (e.g., `execApiCurlCommand baseUri $uri host $h ...`). Procedures declared with `proc` use **positional parameters** in the standard Tcl fashion. All `nproc` procedures in this package are:

* `makeApiCall`
* `execApiCurlCommand`
* `execInternalApiCurlCommand`
* `execPublicApiCurlCommand`
* `getViaJsonPaths`

### External command execution

All external process invocations follow the same three-step idiom:

```tcl
set command [eval buildExecCommand false true <tool> <args...>]
set savedPreviousPid [info previouspid]
try {
  evalExecCommand $command
} finally {
  info previouspid true $savedPreviousPid
}
```

`buildExecCommand` prepares the command list (the two boolean flags control stdin piping and sudo elevation). `evalExecCommand` runs it and returns stdout. The `savedPreviousPid` save/restore ensures the caller's `[info previouspid]` value is not clobbered.

### Error handling

* Validation errors use `error [appendArgs ...]` to produce a single-string message with context.
* Fallible operations are wrapped in `[catch {...} result] == 0` guards; on success the result is post-processed, on failure it is re-raised with `error $result`.
* The `try`/`finally` form is reserved for cases that need guaranteed cleanup (temporary files, PID restoration).

### Statistics hook pattern

Each system metric in `::afterRecordTestStatistics` follows a repeating block separated by `###` comment lines:

1. Check `::no(query,<name>)` — if set, skip the query.
2. Call the query procedure inside `catch` — on failure, default to an empty list.
3. Store the count in `array(<name>,$index)`.
4. If the metric is in the `listed` set, also store the full list in `array(<name>,$index,list)`.

Adding a new metric means duplicating this block, adding the query procedure, and appending the key in `::beforeReportTestStatistics`.

### Tcl style notes

* `then` is used in `if` statements for readability (optional in Tcl).
* Empty-string tests use `[string length $x] == 0` / `> 0` rather than `$x eq ""`.
* Variables follow `camelCase` naming. Namespace variables use a `chelsea_` prefix.
* Safe quoting of arguments passed through `eval` uses `[list ...]` wrappers.

---

## API Endpoint Mapping

### `initializeApiMappings force`

* **Purpose:** Load API endpoint mappings from the TSV configuration file.
* **Params:**
  * `force` — if `true`, reload even if already loaded.
* **Side Effects:** Populates `chelsea_api_mappings` namespace variable.
* **File Format:** Tab-separated values with columns:
  * `type` — `public`, `internal`, or `proxy`
  * `name` — endpoint identifier
  * `method` — HTTP verb
  * `path` — URL path (may contain `%id%` placeholder)
  * `postData` — empty or `%json%`
* **Raises:**
  * if any line does not have exactly five fields.
  * if type is not `public`, `internal`, or `proxy`.
  * if name is not a valid identifier.
  * if method is not a recognized HTTP verb.
  * if path is not a valid relative URI.
  * if postData is neither empty nor `%json%`.
* **Notes:** Reads from `<package>/data/apis/mappings.tsv`. Blank lines and lines starting with `#` are skipped.

### `extractTestApiKey`

* **Purpose:** Extract API key from the `public-api.sh` script file.
* **Returns:** API key string, or empty string if not found.
* **Notes:** Uses global `path` variable; looks for `AUTH_TOKEN="..."` pattern.

### `makeApiCall type name id json proxy parameters`

* **Purpose:** Execute an API call using configured endpoint mappings.
* **Params:**
  * `type` — `public`, `internal`, or `proxy`.
  * `name` — endpoint name from mappings.
  * `id` — optional identifier for `%id%` substitution.
  * `json` — optional JSON body for `%json%` substitution.
  * `proxy` — if `true`, route through proxy.
  * `parameters` — optional query parameters.
* **Returns:** Result from `execApiCurlCommand`.
* **Raises:**
  * if type is invalid.
  * if name is not a valid identifier.
  * if endpoint mapping doesn't exist.
  * if endpoint requires id but none provided.
  * if endpoint requires JSON but invalid JSON provided.
  * if proxy is not a valid boolean.
  * if type is `internal` and proxy is `true`.
* **Notes:** Uses globals `proxyApiHost`, `publicApiHost`, `internalApiHost`, `headerHost`.

---

## JSON Template Processing

### `getJsonTemplateFileName type name`

* **Purpose:** Get the path to a JSON template file.
* **Params:**
  * `type` — `public`, `internal`, or `proxy`.
  * `name` — template name (identifier).
* **Returns:** Path to `<package>/data/json/<type>/<name>.jsont`.
* **Raises:** if type or name is invalid.

### `getJsonTemplate type name`

* **Purpose:** Read a JSON template file.
* **Params:**
  * `type` — template type.
  * `name` — template name.
* **Returns:** Template file contents.
* **Raises:** if template file doesn't exist.

### `processJsonTemplate type value`

* **Purpose:** Recursively process a JSON template, expanding `%name%` tokens.
* **Params:**
  * `type` — template type for nested lookups.
  * `value` — template string to process.
* **Returns:** Processed template with tokens replaced.
* **Token Resolution Order:**
  1. If `%name%` matches a template file, include that template (recursive).
  2. If `%name%` matches a variable in caller's scope, substitute its value.
  3. Otherwise, leave token unchanged.
* **Raises:** if token syntax is malformed (unmatched `%`).

### `setupJsonTemplateVariables`

* **Purpose:** Set up common template variables for testing.
* **Side Effects:** Sets in caller's scope:
  * `vm_id` — new GUID
  * `commit_id` — new GUID

### `cleanupJsonTemplateVariables`

* **Purpose:** Clean up template variables from caller's scope.
* **Side Effects:** Unsets `vm_id`, `commit_id`.

---

## API cURL Wrappers

### `execApiCurlCommand baseUri host apiKey method path data query`

* **Purpose:** Execute a cURL command for API requests.
* **Returns:** Result from `execCurlCommand`.
* **Notes:** Wrapper that passes through to `execCurlCommand`.

### `execInternalApiCurlCommand host apiKey method path data query`

* **Purpose:** Execute a cURL command against the internal API.
* **Returns:** Result from `execCurlCommand`.
* **Raises:** if `internalApiHost` global is not set.

### `execPublicApiCurlCommand host apiKey method path data query`

* **Purpose:** Execute a cURL command against the public API.
* **Returns:** Result from `execCurlCommand`.
* **Raises:** if `publicApiHost` global is not set.

### `execApiSshCommand ip id withRoot args`

* **Purpose:** SSH to a VM using credentials from the internal API.
* **Params:**
  * `ip` — VM IP address.
  * `id` — VM identifier.
  * `withRoot` — if `true`, execute via sudo.
  * `args` — command to run on the VM.
* **Returns:** Result from `execSshCommand`.
* **Notes:**
  * Fetches SSH key via `writeSshKeyToFile`.
  * Uses port 22.
  * Cleans up temporary key file.

---

## JSON Path Helpers

### `getViaJsonPaths json jpaths channel default quiet`

* **Purpose:** Safely extract a value from JSON using path selectors.
* **Params:**
  * `json` — JSON string.
  * `jpaths` — list of path segments.
  * `channel` — output channel for error messages.
  * `default` — value to return on failure (if provided).
  * `quiet` — if `true`, suppress error output.
* **Returns:** Extracted value, or `default` on failure.
* **Raises:** on failure when no default is provided.

---

## System Environment Queries

### `getServiceFiles`

* **Purpose:** Collect service-related files and sockets from common locations.
* **Returns:** List of file paths.
* **Patterns Searched:**
  * `/dev/chelsea`
  * `/root/.local/share/chelsea-manager/*`
  * `/root/.ssh/known_hosts`
  * `/tmp/vm-host.sock`
  * `/tmp/Dockerfile.test`
  * `/tmp/boatswain/*`
  * `/tmp/chelseasnap_*`
  * `/tmp/upload_*.tar`
  * `/tmp/tar_*`
  * `/tmp/dockerdump_*.tar`
  * `/tmp/*-snapshot`
  * `/tmp/*-memory`
  * `/tmp/<guid>`
  * `/tmp/<guid>.json`
  * `/tmp/vm-<guid>.sock`
  * `/tmp/esc_*`
  * `/tmp/etd_*`
  * `/var/run/user/<uid>/tmp/esc_*`
  * `/var/run/user/<uid>/tmp/etd_*`
* **Notes:** Uses `sudo find` for paths under `/root/`.

### `matchRunningProcesses name`

* **Purpose:** Find PIDs of running processes by name.
* **Params:**
  * `name` — process name to match.
* **Returns:** List of PID strings.
* **Notes:** Uses `ps -C <name> -o pid=`.

### `getChelseaProcesses`

* **Purpose:** Get PIDs of running Chelsea daemon processes.
* **Returns:** List of PIDs from `matchRunningProcesses chelsea`.

### `getFirecrackerProcesses`

* **Purpose:** Get PIDs of running Firecracker processes.
* **Returns:** List of PIDs from `matchRunningProcesses firecracker`.

### `getLogicalVolumes`

* **Purpose:** List device-mapper logical volumes.
* **Returns:** List of volume entries from `dmsetup ls`.

### `getLoopDevices`

* **Purpose:** List loop devices.
* **Returns:** List of device mappings from `losetup -n -a --list`.

### `getNetworkNamespaces`

* **Purpose:** List network namespaces.
* **Returns:** List of namespace names from `ip netns list`.

### `getNetworkDevices`

* **Purpose:** List network devices with namespace associations.
* **Returns:** List of `link-netns` associations from `ip link show`.

### `getWireGuardInterfacesForNamespace namespace`

* **Purpose:** List WireGuard interfaces within a specific network namespace.
* **Params:**
  * `namespace` — network namespace name; if empty (default), queries the default namespace.
* **Returns:** List of WireGuard interface names.
* **Notes:** Runs `ip [-n <namespace>] -o link show type wireguard` and parses interface names from the output.

### `getWireGuardInterfaces`

* **Purpose:** List all WireGuard interfaces across all network namespaces.
* **Returns:** Combined list of WireGuard interface names from the default namespace and every namespace returned by `getNetworkNamespaces`.
* **Notes:** Calls `getWireGuardInterfacesForNamespace` once with no argument (default namespace) and then once per namespace from `getNetworkNamespaces`.

---

## Test Hooks

### `::testFailure`

* **When:** Invoked automatically when a test fails.
* **Behavior:**
  * Determines the failing test name (via `upvar` on `testName(true)`).
  * If `+breakOnTestFailure` runtime option is set, breaks into the script debugger.
  * Disables subsequent cleanup unless `::no(NoDisableCleanupForTest)` or `::env(NoDisableCleanupForTest)` is set.
* **Side Effects:**
  * Sets `::env(NoCleanupEnabled)` with test name and timestamp.
  * Sets `::env(NO_FETCH_FS_MOVE)` to `1` (copy instead of move).
  * Sets `::env(NO_FETCH_FS_CLEANUP)` to `1` (suppress deletion).
* **Troubleshooting CLI Options** (from the source comment block):
  * `-runtimeoption +cleanupDisabled` — disable all test cleanup globally.
  * `-runtimeoption +stopDisabled` — prevent the test harness from stopping.
  * `-runtimeoption +breakOnTestFailure` — break into the debugger on failure.
  * `-match chelsea-X.Y` — run only a specific test (placed after `-file`).
  * `-constraints firecrackerBuild` — force a test constraint to be present.

### `::afterRecordTestStatistics`

* **When:** Hook after per-test statistics are recorded. Only runs when `index` is `before` or `after` (via `upvar 2`).
* **Collects Counters For:**
  * `serviceFiles`
  * `chelseaProcesses`
  * `firecrackerProcesses`
  * `logicalVolumes`
  * `loopDevices`
  * `networkNamespaces`
  * `networkDevices`
  * `wireGuardInterfaces`
* **Notes:**
  * Uses `::no(query,<name>)` to skip specific queries.
  * Optionally stores full lists in `array(<name>,<index>,list)`.

### `::beforeReportTestStatistics`

* **When:** Hook before statistics are reported.
* **Behavior:** Adds Chelsea statistics keys to the `statistics` list.

---

## Test Settings

### `maybeLoadTestSettings quiet`

* **Purpose:** Load test settings from `settings.eagle` in the test path.
* **Params:**
  * `quiet` — if `true`, suppress error messages.
* **Returns:** `true` if loaded successfully, `false` otherwise.
* **Notes:**
  * Uses global `path` variable.
  * Initializes `commonConstraints` to `{eagle}` if not set.

### `cleanupTestSettings`

* **Purpose:** Clean up test settings variables.
* **Side Effects:** Unsets:
  * `exeFile`, `wireGuardFile`, `commonConstraints`
  * `proxyApiHost`, `publicApiHost`, `internalApiHost`, `headerHost` (if set by test)
  * `env(getDefaultTestLog)`

---

## Idiomatic Usage Patterns

### Making an API call with a JSON body

The typical sequence for constructing and issuing an API request:

```tcl
initializeApiMappings
setupJsonTemplateVariables

set template [getJsonTemplate public create_vm]
set body     [processJsonTemplate public $template]
set result   [makeApiCall public create_vm "" $body]

cleanupJsonTemplateVariables
```

`setupJsonTemplateVariables` creates `vm_id` and `commit_id` in the caller's scope so that `processJsonTemplate` can resolve the corresponding `%vm_id%` and `%commit_id%` tokens.

### Inspecting system state before and after a test

The `::afterRecordTestStatistics` hook automatically snapshots the environment, but individual queries can be called directly for assertions:

```tcl
set before [getWireGuardInterfaces]
# ... run test that creates a WireGuard interface ...
set after  [getWireGuardInterfaces]
# assert [llength $after] == [expr {[llength $before] + 1}]
```

### Settings lifecycle

Test scripts typically bracket their execution with:

```tcl
maybeLoadTestSettings          ;# prologue — loads settings.eagle
# ... tests ...
cleanupTestSettings            ;# epilogue — unsets globals
```

`maybeLoadTestSettings` sources the settings file in the caller's scope (via `uplevel 1`), so variables it defines (e.g., `exeFile`, `publicApiHost`) become available as globals.

### Suppressing individual statistics queries

To skip an expensive or irrelevant query during a test run, set the corresponding `::no` variable before the test executes:

```tcl
set ::no(query,logicalVolumes) 1   ;# skip dmsetup ls
set ::no(query,loopDevices) 1      ;# skip losetup
```

---

## Dependencies & Globals

* **Namespace variables** (`::Chelsea`):
  * `chelsea_api_mappings` — array keyed by `<type>,<name>` holding endpoint definitions.
  * `chelsea_package_directory` — root path of the `Chelsea` package on disk.
* **Globals:**
  * `path` — test path
  * `proxyApiHost`, `publicApiHost`, `internalApiHost` — API endpoints
  * `headerHost` — custom Host header
  * `commonConstraints` — test constraints
* **Other Chelsea helpers:** `execCurlCommand`, `writeSshKeyToFile`, `execSshCommand`, `isCleanupEnabled`, `getOrSetViaJsonPaths`, `isValidIdentifier`, `isValidId`, `isValidJson`, `getHttpVerbs`, `globForUniqueIdentifier`, `getUid`, `buildExecCommand`, `evalExecCommand`.
* **Eagle builtins used:** `readFile`, `appendArgs`, `tputs`, `hasRuntimeOption`, `guid new`, `uri isvalid`.
* **External tools:** `ps`, `dmsetup`, `losetup`, `ip`, `find`, `curl` (via `execCurlCommand`), `ssh` (via `execSshCommand`).

---

*Package:* `Chelsea.Test 1.0` · *Namespace:* `::Chelsea` (hooks in `::`)
