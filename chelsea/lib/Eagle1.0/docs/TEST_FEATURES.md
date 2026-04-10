# Eagle Test Harness Features Used in This Repository

> **Scope.** This document covers the **Eagle.Test** harness and related utilities used by the repository’s test suites (e.g., `basic.eagle`, `test.eagle`). It focuses on commands, hooks, and options that are not present in stock Tcl’s `tcltest`.

---

## Table of Contents

- [Suite Lifecycle](#suite-lifecycle)
  - [`runTestPrologue` / `runTestEpilogue`](#runtestprologue--runtestepilogue)
  - [`runTest` structure](#runtest-structure)
- [Constraints](#constraints)
  - [`addConstraint`, `haveConstraint`, `checkForFile`](#addconstraint-haveconstraint-checkforfile)
- [Process Execution: `testExec`](#process-execution-testexec)
  - [Standard capture & tracing](#standard-capture--tracing)
  - [Quoting & shelling patterns](#quoting--shelling-patterns)
- [Harness Logging](#harness-logging)
  - [`tputs`, `tlog`, and channels](#tputs-tlog-and-channels)
- [Hooks & Failure Handling](#hooks--failure-handling)
  - [`::testFailure`](#testfailure)
  - [`::afterRecordTestStatistics`](#afterrecordteststatistics)
  - [`::beforeReportTestStatistics`](#beforereportteststatistics)
- [Timing, PIDs, and Waits](#timing-pids-and-waits)
  - [`waitForProcesses`](#waitforprocesses)
  - [`info previouspid`](#info-previouspid)
  - `::test_timeout`
- [JSON Utilities](#json-utilities)
  - [`isValidJson`, `getOrSetViaJsonPaths`](#isvalidjson-getorsetviajsonpaths)
  - Project wrappers: `getViaJsonPaths`, `extractJson`, `isSuccessJson`
- [Other Utilities Commonly Used by Tests](#other-utilities-commonly-used-by-tests)
  - `getTestLog`, `getDefaultTestLog`
  - `appendArgs`, `readFile`, `writeFile`
- [Common Patterns from This Repository](#common-patterns-from-this-repository)

---

## Suite Lifecycle

### `runTestPrologue` / `runTestEpilogue`

Bracket the test run with harness setup/teardown (channels, logging, environment).

### `runTest` structure

Each test is declared as a single `runTest {...}` block with declarative sections:

```tcl
runTest {
  test chelsea-1.2 {fetch file system}
  -setup    { ... }       ;# optional
  -body     { ... }       ;# required
  -cleanup  { ... }       ;# optional
  -constraints $common    ;# skip if unmet
  -match     {exact|glob|regexp}
  -result    {... expected dict or pattern ...}
}
```

- **`-constraints`**: If any listed constraint is missing, the test is **skipped**.
- **`-match`**: Choose how to compare actual vs expected.

---

## Constraints

### `addConstraint`, `haveConstraint`, `checkForFile`

- `addConstraint name` — register an environmental capability (e.g., `firecrackerBuild`).
- `haveConstraint name` — query presence; used to guard setup.
- `checkForFile $channel $path` — if `$path` exists, adds `file_<tail>` constraint and logs on `$channel`.

The suite uses constraints to gate tests on required artifacts (`Makefile`, `api.sh`, `commands.sh`, daemon binary) and environment (`firecrackerBuild`).

---

## Process Execution: `testExec`

Shell out to external programs with structured capture of `exitCode`, `stdOut`, and `stdErr`.

```tcl
testExec \
  -exitcode exitCode \
  -stdout   stdOut \
  -stderr   stdErr \
  ?-debug? ?-trace? \
  -- program arg1 arg2 ...
```

The project wraps this with its own helpers (defined in this repository, not part of the Eagle distribution) that compose commands, e.g. `buildExecCommand`, and normalize results via `formatExecResults`:

```tcl
# Expect: {exitCode <enum> stdOut <trimmed> stdErr <trimmed>}
set result [formatExecResults]
```

### Standard capture & tracing

When the runtime option `auditExec` is enabled, the wrappers pass `-debug -trace` to `testExec` to surface command lines and environment.

### Quoting & shelling patterns

The repository's `appendExecArgs` (named `eagle_appendExecArgs` in the official Eagle script library) handles Eagle-friendly quoting and supports both direct execution and `SHELL -c ...` when `viaShell` is true.

---

## Harness Logging

### `tputs`, `tlog`, and channels

- `tputs $channel $message` — primary test output; writes to channel and log file. Suppresses repeated consecutive output.
- `tlog  $message` — writes to the test log file only (not to a channel).
- `$test_channel` — canonical harness channel variable (passed to helpers).

---

## Hooks & Failure Handling

### `::testFailure`

Called by the harness when any test fails. This project’s implementation:

- Announces which test failed (if known).
- If `+breakOnTestFailure` is set, **breaks into the interactive debugger** via `debug emergency {now break}`.
- Disables subsequent cleanup by setting environment flags (`NoCleanupEnabled`, `NO_FETCH_FS_MOVE`, `NO_FETCH_FS_CLEANUP`) so artifacts persist for debugging.

### `::afterRecordTestStatistics`

Called after per-test statistics are written. This project augments stats with counts (and optionally lists) of:

- service files (globs under `/tmp`, `/root`, jailer dirs, etc.)
- running processes (`chelsea`, `firecracker`)
- logical volumes (`dmsetup`)
- loop devices (`losetup`)
- network namespaces (`ip netns list`)
- network devices (`ip link show`)
- WireGuard interfaces (`ip -o link show type wireguard`, across all namespaces)

Presence of `::no(query,...)` variables can skip probes.

### `::beforeReportTestStatistics`

Ensures the above statistic names are included in the final report.

---

## Timing, PIDs, and Waits

### `waitForProcesses`

Wait for child PIDs up to a timeout (ms). Used to wait for daemon shutdown.

### `info previouspid`

The harness tracks the “most recently created process”. The project saves/restores it around `testExec` calls that run privileged utilities so the harness view stays consistent:

```tcl
set saved [info previouspid]
# run a privileged tool via testExec
info previouspid true $saved
```

- `info previouspid` → get
- `info previouspid true $pid` → set

`::test_timeout` (if set) controls waits in several flows.

---

## JSON Utilities

### `isValidJson`, `getOrSetViaJsonPaths`

Convenience helpers supplied with Eagle tooling to validate and project into JSON documents. The JSON path is expressed as a Tcl list, e.g.:

```tcl
getOrSetViaJsonPaths $json [list data vms 0 id]
getOrSetViaJsonPaths $json [list status]
```

**Project wrappers used here**

- `getViaJsonPaths` — safe accessor with defaulting and channel logging.
- `extractJson` — strips known progress text and returns the JSON segment from mixed output.
- `isSuccessJson` — heuristic success: no `"error"`, and either `"data"` present or `"status" == "Completed"`.

---

## Other Utilities Commonly Used by Tests

- `getTestLog` / `getDefaultTestLog` — locate the current and default test logs; used to seed nested Eagle shells (via `env(getDefaultTestLog)`).
- `appendArgs`, `readFile`, `writeFile` — small utilities widely used by the project’s helpers.

---

## Common Patterns from This Repository

- **Constraint-driven gating.** Tests list only what they require; unmet capabilities skip the test instead of failing the suite.
- **Daemon lifecycle.** Helpers (`startDaemon`, `setupDaemon`, `stopDaemon`, `cleanupDaemon`) orchestrate build/copy/init/start/stop with optional `strace` controlled by runtime options.
- **JSON-first assertions.** Admin commands sometimes print progress plus JSON; API commands print pure JSON. The suite extracts/validates JSON and asserts fields by path.
- **Best-effort cleanup.** `cleanupViaApi` attempts normal then forced deletion of test resources; `::testFailure` disables cleanup to aid debugging.
- **SSH with ephemeral keys.** `execApiSshCommand` fetches a one-off private key from the API and runs remote commands with strict host key checking disabled for ephemeral VMs.
