# How to Write a Chelsea Test (with Eagle and the test-suite packages)

## 0) Prerequisites & mental model

* You’re writing **Eagle** scripts (Tcl-like) that run under **Eagle.Test**.
* Prefer the **helpers** (wrappers) so you get consistent logging, quoting, environment, cleanup, JSON validation, etc. Frequent helpers include:

  * **Process & env:** `maybeCreateTemporaryDirectory`, `maybeUseTemporaryDirectory`, `maybeDeleteTemporaryDirectory`, `makeSureChelseaIsNotRunning`
  * **Daemon lifecycle:** `startDaemon`, `setupDaemon`, `stopDaemon`, `cleanupDaemon`
  * **Administration/API CLIs:** `execMakeCommand`, `execAdminCommand`, `execApiCommand`, `testExecApiCommand`
  * **HTTP/SSH:** `execApiCurlCommand`, `execApiSshCommand`
  * **JSON:** `extractJson`, `getViaJsonPaths`, `isSuccessJson`
  * **IDs & validation:** `isValidClusterId`, `isValidVirtualMachineId`, `verifyMachineAndClusterIds`
  * **Miscellaneous:** `getVarOrNull`, `normalizeSpaces`, `getAdminApiKey`, `checkForFirecrackerBuild`

---

## 1) Create a new test file

Create `tests/myfeature.eagle` with this **skeleton**:

```tcl
###############################################################################
# myfeature.eagle -- Tests for <describe feature briefly>
###############################################################################

package require Eagle
package require Eagle.Test
runTestPrologue

# Project packages (helpers)
package require Chelsea.Meta

# Safety: ensure nothing from a previous run is in the way
makeSureChelseaIsNotRunning

# Per-suite temporary root & wiring for DATA_DIR/DB_PATH
maybeCreateTemporaryDirectory
maybeUseTemporaryDirectory

# Make nested Eagle shells share this log file
set env(getDefaultTestLog) [getTestLog]

# Default API host if not provided by the invoker
if {![info exists apiHost]} then {
  set apiHost http://0.0.0.0
  set setApiHost true
}

# Point to local build artifacts (adjust as needed for your repo layout)
set makeFile     [file join [file dirname $path] Makefile]
set commandsFile [file join [file dirname $path] commands.sh]
set apiFile      [file join [file dirname $path] api.sh]
set exeFile      [file join [file dirname $path] target release chelsea]

# Constraints: gate tests on required files / environment
if {![haveConstraint [appendArgs file_ [file tail $makeFile]]]}     then {checkForFile $test_channel $makeFile}
if {![haveConstraint [appendArgs file_ [file tail $commandsFile]]]} then {checkForFile $test_channel $commandsFile}
if {![haveConstraint [appendArgs file_ [file tail $apiFile]]]}      then {checkForFile $test_channel $apiFile}
if {![haveConstraint [appendArgs file_ [file tail $exeFile]]]}      then {checkForFile $test_channel $exeFile}
if {![haveConstraint firecrackerBuild]} then {checkForFirecrackerBuild $test_channel}

# Common set of constraints most tests will require
set commonConstraints [list eagle testExec file_Makefile file_chelsea file_commands.sh file_api.sh]

# ... your runTest blocks go here ...

# Suite epilogue: cleanup + unset transient variables
unset -nocomplain makeFile commandsFile apiFile exeFile commonConstraints
if {[info exists setApiHost]} then {unset -nocomplain apiHost setApiHost}
unset -nocomplain env(getDefaultTestLog)
maybeDeleteTemporaryDirectory

runTestEpilogue
```

**Why these pieces matter**

* **Prologue/Epilogue**: standard harness setup/teardown.
* **Temporary directory helpers**: create a **safe** workspace and set `chelsea_data_directory` and `chelsea_database_path` for the helpers.
* **Constraints**: allow **skipping** tests when required artifacts aren’t present (vs failing).
* **`env(getDefaultTestLog)`**: ensures child Eagle shells inherit the same log.

---

## 2) Write a simple CLI test (Makefile or usage)

This exercises **process wrappers** and result normalization.

```tcl
runTest {test myfeature-0.1 {make target: nop} -body {
  execMakeCommand false nop
} -cleanup {
  catch {info previouspid true}
} -constraints $commonConstraints -result \
{exitCode Success stdOut nop stdErr {}}}
```

**Helpers used**

* `execMakeCommand withRoot args` → runs `make -s -f $makeFile -- <args>` in the correct directory.
* Internally, the helpers normalize to `{exitCode stdOut stdErr}` so your `-result` comparison is stable.

**Pattern:** prefer wrappers instead of raw `exec` for predictable capture, logging, and quoting.

---

## 3) Assert CLI usage output (administration / API tools)

```tcl
runTest {test myfeature-0.2 {administration usage banner} -body {
  execAdminCommand
} -cleanup {
  catch {info previouspid true}
} -constraints $commonConstraints -match glob -result \
{exitCode Failure stdOut {Usage: *} stdErr {}}}
```

* `execAdminCommand` runs `commands.sh` without arguments (should print usage).
* `-match glob` tolerates variations in the banner.

---

## 4) Start the daemon for DB-backed administration tests

Use the **lifecycle helpers**; they manage building/copying artifacts, `.env` generation, DB init, and process start.

```tcl
runTest {test myfeature-1.0 {list API keys} -setup {
  startDaemon        ;# may init DB if necessary
} -body {
  normalizeSpaces [execAdminCommand list-api-keys]
} -cleanup {
  catch {stopDaemon}
  catch {info previouspid true}
} -constraints $commonConstraints -match regexp -result \
{^\{exitCode Success stdOut \{Listing all API keys:(?: [0-9a-f]{32})*\} stdErr \{\}\}$}}
```

**Helpers used**

* `startDaemon` / `stopDaemon`
* `normalizeSpaces` for resilient output matching
* `execAdminCommand` to run administration CLI verbs

---

## 5) Round-trip an administration flow (create/delete key)

Use `regexp` to extract values from CLI output; assert final state.

```tcl
runTest {test myfeature-1.1 {create+delete API key} -setup {
  startDaemon
} -body {
  set raw1   [execAdminCommand create-api-key]
  set line1  [getDictionaryValue $raw1 stdOut]

  if {![regexp -skip 1 -- { ([0-9a-f]{32})$} $line1 id]} then {
    error [appendArgs "could not create API key: " $raw1]
  }

  set raw2   [execAdminCommand delete-api-key $id]
  set line2  [getDictionaryValue $raw2 stdOut]

  if {$line2 ne "API key deleted successfully"} then {
    error [appendArgs "could not delete API key: " $raw2]
  }

  normalizeSpaces [execAdminCommand list-api-keys]
} -cleanup {
  catch {stopDaemon}
  catch {info previouspid true}
  unset -nocomplain id raw1 raw2 line1 line2
} -constraints $commonConstraints -result [string map [list \
%admin_api_key% [getAdminApiKey true]] {exitCode Success stdOut {Listing all\
API keys: %admin_api_key%} stdErr {}}]}
```

**Pattern:** manipulate the CLI via helpers; verify postcondition; interpolate expected content using `getAdminApiKey true`.

---

## 6) Administration commands that return mixed output + JSON

Use `extractJson` + `getViaJsonPaths` to validate structured results.

```tcl
runTest {test myfeature-1.2 {fetch fs completes} -setup {
  startDaemon
} -body {
  set out  [execAdminCommand fetch-fs]
  set txt  [getDictionaryValue $out stdOut]
  set json [extractJson $txt]

  if {![isValidJson $json]} then { error "bad JSON: $txt" }

  if {[getViaJsonPaths json $json jpaths [list status]] ne "Completed"} then {
    error [appendArgs "bad status: " $json]
  }

  if {[string length [getViaJsonPaths json $json jpaths [list error]]] != 0} then {
    error [appendArgs "unexpected error: " $json]
  }

  if {[getViaJsonPaths json $json jpaths [list type]] ne "path"} then {
    error [appendArgs "bad type: " $json]
  }

  if {[string is not directory -strict [getViaJsonPaths json $json jpaths [list data]]]} then {
    error [appendArgs "bad data path: " $json]
  }
} -cleanup {
  catch {stopDaemon}
  catch {info previouspid true}
} -constraints $commonConstraints -result {}}
```

**Helpers used**

* `extractJson` (strip progress text)
* `getViaJsonPaths` (safe JSON projection + optional logging)
* `isValidJson`

---

## 7) Pure API CLI tests (JSON-first)

Prefer `testExecApiCommand` so you get **`stdOut` extraction → JSON validation → path selection** in one call.

```tcl
runTest {test myfeature-2.0 {list clusters is empty} -setup {
  startDaemon createApiKey true
} -body {
  if {[testExecApiCommand \
        command list-clusters jpaths [list data]] ne "\[\]"} then {
    error "expected empty list"
  }
} -cleanup {
  catch {stopDaemon}
  catch {info previouspid true}
} -constraints $commonConstraints -result {}}
```

**Helpers used**

* `testExecApiCommand command <verb> jpaths <pathList> ?varName json? ?args {...}?`

---

## 8) Full cluster lifecycle (create + verify + cleanup)

This combines several high-level helpers and shows the **upvar** patterns they use.

```tcl
runTest {test myfeature-2.3 {create cluster + verify IDs} -setup {
  setupDaemon     ;# start, fetch-kernel, fetch-fs
} -body {
  createCluster varName ids   ;# sets ids(machine), ids(cluster), ids(address)

  # cross-check machine id via list-clusters
  set ids(machine,0) [testExecApiCommand \
      command list-clusters jpaths [list data 0 vms 0 id] varName json]

  verifyMachineAndClusterIds \
      jsonVarName json idsVarName ids machineName 0 \
      clusterJPaths [list data 0 id]

  # verify cluster id via get-cluster
  set ids(cluster,0) [testExecApiCommand \
      command get-cluster jpaths [list data id] varName json args [list $ids(cluster)]]

  if {$ids(cluster) ne $ids(cluster,0)} then { error "cluster id mismatch" }

  cleanupViaApi $test_channel cluster $ids(cluster)
} -cleanup {
  catch {cleanupDaemon id [getVarOrNull ids(cluster)]}
  unset -nocomplain ids json
} -constraints $commonConstraints -result {true}}
```

**Helpers used**

* `setupDaemon` (calls `startDaemon createApiKey true` + fetches)
* `createCluster varName ids`
* `testExecApiCommand` for reads
* `verifyMachineAndClusterIds` (compares GUIDs/Base58 ids with selectable operator)
* `cleanupViaApi $channel cluster <id>` (best-effort: normal → forced)
* `cleanupDaemon id [getVarOrNull ids(cluster)]` (safe cleanup if setup partially failed)

**Pattern:** put **the cluster id** into your `-cleanup` via `getVarOrNull` so teardown never errors when setup didn’t complete.

---

## 9) Branch a VM (machine id must differ; cluster must match)

```tcl
runTest {test myfeature-2.4 {branch VM changes machine id} -setup {
  setupDaemon
} -body {
  createCluster varName ids

  set ids(machine,0) [testExecApiCommand \
      command branch-vm jpaths [list data id] varName json args [list $ids(machine)]]

  verifyMachineAndClusterIds \
      jsonVarName json idsVarName ids machineName 0 \
      machineOperator ne clusterJPaths [list data cluster_id]

  list true
} -cleanup {
  catch {cleanupDaemon id [getVarOrNull ids(cluster)]}
  unset -nocomplain ids json
} -constraints $commonConstraints -result {true}}
```

**Key idea:** `verifyMachineAndClusterIds` lets you **choose the comparator** (`eq` or `ne`) for machine ids and separately compare cluster ids.

---

## 10) SSH into the VM (ephemeral key via API)

**Use `execApiSshCommand`** so the test fetches a transient private key securely and cleans it up.

```tcl
runTest {test myfeature-2.6 {ssh uname -r} -setup {
  setupDaemon
} -body {
  createCluster varName ids

  set r [execApiSshCommand $ids(address) $ids(machine) true {uname -r}]
  set rc   [getDictionaryValue $r exitCode]
  set out  [getDictionaryValue $r stdOut]
  set err  [getDictionaryValue $r stdErr]

  if {$rc ne "Success"} then { error [appendArgs "ssh failed: " $err] }

  tputs $test_channel [appendArgs "---- VM kernel: " [string trim $out] "\n"]
} -cleanup {
  catch {cleanupDaemon id [getVarOrNull ids(cluster)]}
  unset -nocomplain ids r rc out err
} -constraints $commonConstraints -result {}}
```

**Helpers used**

* `execApiSshCommand ip machineId withRoot args…`

  * Fetches the key (`writeSshKeyToFile`) → calls `ssh` with strong non-interactive options → deletes the key in `finally`.

---

## 11) Result matching strategies

* **Exact**: the returned result must match the expected result verbatim.
* **Glob**: loose banners (`Usage: *`).
* **Regexp**: structured content (e.g., lists of IDs, file sizes, kernel versions).
* Use `normalizeSpaces` when comparing human-readable text from CLIs.

---

## 12) Runtime options for debugging

These knobs change behavior **without touching tests** (pass as `-runtimeoption +flag`):

* `+cleanupDisabled`: preserve artifacts; `::testFailure` also sets this automatically on failure.
* `+stopDisabled`: don’t stop the daemon in `stopDaemon` (use sparingly).
* `+breakOnTestFailure`: drop into the interactive debugger (`debug emergency {now break}`) on the first failing test.
* `+straceDaemon`: wrap daemon start with `strace` (log file determined by `getTestStraceLog`).

---

## 13) Common pitfalls & how helpers avoid them

* **Quoting & shelling**: Don’t build raw command lines; use `exec…` helpers (they quote; some can run via `$SHELL -c` when needed).
* **PID tracking**: The harness uses `info previouspid`; wrappers save/restore it so wait logic and resource leak reporting stay consistent.
* **Unsafe cleanup**: Filesystem helpers only delete temp dirs that match **known-safe** patterns; the test hook `::testFailure` disables cleanup to retain evidence.
* **JSON extraction**: Administration commands may prepend progress text; always run through `extractJson` + `isValidJson` before asserting paths.

---

## 14) Checklist for a new test

1. **Add skeleton** with `runTestPrologue/Epilogue`, require packages.
2. **Guard** with `makeSureChelseaIsNotRunning`.
3. **Create temp space**: `maybeCreateTemporaryDirectory` + `maybeUseTemporaryDirectory`.
4. **Propagate log**: `set env(getDefaultTestLog) [getTestLog]`.
5. **Compute paths** to `Makefile`, `commands.sh`, `api.sh`, `chelsea` binary.
6. **Add constraints**: `checkForFile` and `checkForFirecrackerBuild` as appropriate.
7. **Define `$commonConstraints`** for your suite.
8. **Write tests** using the wrappers:

   * CLI usage → `execAdminCommand` / `execApiCommand`
   * Daemon flows → `startDaemon` / `setupDaemon` / `stopDaemon` / `cleanupDaemon`
   * JSON assertions → `extractJson`, `getViaJsonPaths`, `testExecApiCommand`
   * Cluster/VM → `createCluster`, `verifyMachineAndClusterIds`, `cleanupViaApi`
   * SSH → `execApiSshCommand`
9. **Always have cleanup**: use `getVarOrNull` to safely reference IDs in `-cleanup`.
10. **Finish** with `maybeDeleteTemporaryDirectory` and unsets.

---

## 15) Minimal end-to-end example (copy-paste ready)

```tcl
runTest {test myfeature-3.0 {cluster create + uname -r} -setup {
  setupDaemon
} -body {
  # 1) Create a cluster
  createCluster varName ids

  # 2) Verify we can see it from the list API
  set out1 [testExecApiCommand command list-clusters jpaths [list data 0 id]]

  if {$out1 ne $ids(cluster)} then {
    error "unexpected cluster id in list"
  }

  # 3) SSH into the VM and run uname -r
  set out2 [execApiSshCommand $ids(address) $ids(machine) true {uname -r}]
  set exitCode [getDictionaryValue $out2 exitCode]
  set stdOut [string trim [getDictionaryValue $out2 stdOut]]
  set stdErr [string trim [getDictionaryValue $out2 stdErr]]

  if {$exitCode ne "Success"} then {
    error [appendArgs "uname failed: " $stdErr]
  }

  tputs $test_channel [appendArgs "---- VM kernel: " $stdOut "\n"]

  list ok
} -cleanup {
  # Try to delete cluster even if the body bailed out early
  catch {cleanupDaemon id [getVarOrNull ids(cluster)]}
  unset -nocomplain ids out1 out2 exitCode stdOut stdErr
} -constraints $commonConstraints -result {ok}}
```

---

Use this as your playbook. By sticking to the helpers, your tests will be **short, robust, and consistent**—and they’ll automatically benefit from the harness’s logging, safety checks, and debugging hooks.
