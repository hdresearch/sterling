# Chelsea Shell Helpers

Procedures in the `::Chelsea` namespace for shell command execution, argument building, and file ownership management. Provides a foundation for executing external commands throughout the test suite.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## Execution Options

### `getExecOptions viaShell`

* **Purpose:** Get standard options for `testExec` command execution.
* **Params:**
  * `viaShell` — boolean (currently unused, for future shell-specific options).
* **Returns:** List of execution options: `-exitcode exitCode -stdout stdOut -stderr stdErr`.
* **Notes:** If `auditExec` runtime option is set, adds `-debug -trace`.
* **Example:**

  ```tcl
  set opts [::Chelsea::getExecOptions false]
  # {-exitcode exitCode -stdout stdOut -stderr stdErr}
  ```

---

## Command Building

### `appendExecArgs viaShell varName args`

* **Purpose:** Append arguments to a command with proper escaping and quoting.
* **Params:**
  * `viaShell` — if `true`, wrap command for shell execution via `$SHELL -c`.
  * `varName` — variable name to append to (upvar).
  * `args` — arguments to append.
* **Side Effects:** Modifies the variable in caller's scope.
* **Notes:**
  * Escapes backslashes and double quotes.
  * Quotes arguments containing whitespace, quotes, or backslashes.
  * When `viaShell` is true, wraps entire command for shell execution with proper single-quote escaping.

### `buildExecCommand viaShell withRoot fileName args`

* **Purpose:** Build a complete `testExec` command with options.
* **Params:**
  * `viaShell` — if `true`, execute via shell.
  * `withRoot` — if `true`, prepend `sudo`.
  * `fileName` — the executable to run.
  * `args` — arguments for the command.
* **Returns:** Complete command list ready for `eval`.
* **Example:**

  ```tcl
  set cmd [::Chelsea::buildExecCommand false true chmod -R 755 /path]
  # {testExec sudo {-exitcode exitCode ...} chmod -R 755 /path}

  set cmd [::Chelsea::buildExecCommand false false git status]
  # {testExec git {-exitcode exitCode ...} status}
  ```

---

## Command Execution

### `evalExecCommand command`

* **Purpose:** Execute a command built with `buildExecCommand`.
* **Params:**
  * `command` — the command list to execute.
* **Returns:** Result of the executed command.
* **Notes:**
  * Sets `::test_log` from `getDefaultTestLog` if not present.
  * Cleans up `::test_log` and `::set_test_log` after execution.
* **Example:**

  ```tcl
  set cmd [::Chelsea::buildExecCommand false false ls -la]
  ::Chelsea::evalExecCommand $cmd
  ```

### `evalInDirectory directory command`

* **Purpose:** Execute a command in a specific directory.
* **Params:**
  * `directory` — directory to change to (empty string = stay in current).
  * `command` — the command to execute via `uplevel 1`.
* **Returns:** Result of the executed command.
* **Notes:** Restores original working directory after execution.
* **Example:**

  ```tcl
  set result [::Chelsea::evalInDirectory /tmp {exec ls -la}]
  ```

### `formatExecResults`

* **Purpose:** Format execution results into a standard dictionary.
* **Returns:** Dictionary with `exitCode`, `stdOut`, and `stdErr` (trimmed).
* **Notes:** Must be called in scope where `exitCode`, `stdOut`, `stdErr` are set.
* **Example:**

  ```tcl
  set cmd [::Chelsea::buildExecCommand false false git status]
  ::Chelsea::evalExecCommand $cmd
  set results [::Chelsea::formatExecResults]
  # {exitCode Success stdOut "..." stdErr ""}
  ```

---

## User/Group Operations

### `getUidAndGidForUserName userName`

* **Purpose:** Get the UID and GID for a username.
* **Params:**
  * `userName` — the username to look up.
* **Returns:** Dictionary with `uid` and `gid`.
* **Raises:** if `id` command output is malformed.
* **Example:**

  ```tcl
  set ids [::Chelsea::getUidAndGidForUserName root]
  # {uid 0 gid 0}
  ```

### `getUid`

* **Purpose:** Get the current user's UID.
* **Returns:** The UID as an integer.
* **Raises:** if `USER` environment variable is not set.
* **Notes:** Uses `$::env(USER)` to determine current user.

### `changeOwner path userName force`

* **Purpose:** Recursively change ownership of a path.
* **Params:**
  * `path` — file or directory path.
  * `userName` — target owner (also used as group).
  * `force` — if `false`, skip if already owned correctly; if `true`, always change.
* **Returns:** Empty string, or result of chown command.
* **Raises:**
  * if `path` doesn't exist.
  * if `userName` is not a valid username pattern (`^[a-z][\-0-9a-z]*$`).
* **Notes:**
  * Compares current ownership before changing (unless `force`).
  * Uses `sudo chown -R <userName>:<userName> <path>`.
* **Example:**

  ```tcl
  ::Chelsea::changeOwner /var/lib/chelsea root
  ```

---

## Typical Workflow

**Execute a simple command:**

```tcl
set cmd [::Chelsea::buildExecCommand false false uname -a]
::Chelsea::evalExecCommand $cmd
set result [::Chelsea::formatExecResults]
puts "Output: [dict get $result stdOut]"
```

**Execute with sudo:**

```tcl
set cmd [::Chelsea::buildExecCommand false true systemctl restart chelsea]
::Chelsea::evalExecCommand $cmd
```

**Execute in a specific directory:**

```tcl
set cmd [::Chelsea::buildExecCommand false false make build]
::Chelsea::evalInDirectory /path/to/project $cmd
```

---

## Dependencies & Environment

* **Eagle packages:** `testExec` command.
* **Environment:** `$::env(SHELL)` for shell execution, `$::env(USER)` for UID lookup.
* **External tools:** `sudo`, `id`, `chown`.
* **Other helpers:** `getDefaultTestLog`, `getDictionaryValue`, `findFilesRecursive`, `getDirectoryOnly`, `hasRuntimeOption`.

---

*Package:* `Chelsea.Shell 1.0` · *Namespace:* `::Chelsea`
