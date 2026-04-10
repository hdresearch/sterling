# Chelsea Configuration Helpers

Procedures in the `::Chelsea` namespace for managing configuration, environment variables, and `.env` file handling. Used by the test suite and daemon setup.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## Feature Flags

### `isStopEnabled`

* **Purpose:** Check if the test suite is allowed to stop the daemon.
* **Returns:** `true` if stop is enabled, `false` otherwise.
* **Notes:** Returns `false` if any of the following are set:
  * `::no(NoStopEnabled)`
  * `::env(NoStopEnabled)`
  * Runtime option `stopDisabled`
* **Example:**

  ```tcl
  if {[::Chelsea::isStopEnabled]} {
    stopDaemon
  }
  ```

### `isCleanupEnabled`

* **Purpose:** Check if the test suite is allowed to cleanup daemon files.
* **Returns:** `true` if cleanup is enabled, `false` otherwise.
* **Notes:** Returns `false` if any of the following are set:
  * `::no(NoCleanupEnabled)`
  * `::env(NoCleanupEnabled)`
  * Runtime option `cleanupDisabled`
* **Example:**

  ```tcl
  if {[::Chelsea::isCleanupEnabled]} {
    file delete $temporaryFile
  }
  ```

---

## Admin API Key

### `getAdminApiKey noComplain create`

* **Purpose:** Get or create the admin API key for internal service communication.
* **Params:**
  * `noComplain` — if `true`, return empty string instead of raising error when key is missing.
  * `create` — if `true`, generate a new random key if one doesn't exist.
* **Returns:** The API key string (128 hex characters when created).
* **Raises:** if no key exists and both `noComplain` and `create` are `false`.
* **Notes:** Reads from and writes to `::env(ADMIN_API_KEY)`.
* **Example:**

  ```tcl
  set key [::Chelsea::getAdminApiKey false true]
  # Key is now available in $::env(ADMIN_API_KEY)
  ```

---

## Directory Paths

### `getPackageDirectory`

* **Purpose:** Get the directory containing the Chelsea package scripts.
* **Returns:** Absolute path to the package directory.
* **Raises:** if `chelsea_package_directory` variable is not set.

### `getServiceDataDirectory`

* **Purpose:** Get the service data directory path.
* **Returns:** Value of `chelsea_data_directory` if set, otherwise `/var/lib/chelsea`.

---

## Environment Variable Handling

### `expectEnv channel envVarName default`

* **Purpose:** Get an environment variable value, logging warnings if missing or empty.
* **Params:**
  * `channel` — output channel for warning messages.
  * `envVarName` — name of the environment variable.
  * `default` — value to return if variable is missing or empty.
* **Returns:** The environment variable value, or `default`.
* **Notes:** Sets `haveAllEnvVars` to `false` if the variable is missing or empty.

### `setupEnvVarMappings overwrite`

* **Purpose:** Initialize the environment variable mappings for test configuration.
* **Params:**
  * `overwrite` — if `true`, reinitialize even if already set.
* **Notes:** Sets up mappings between script variable names and environment variable names:
  * `testPrivateIp` ↔ `TEST_PRIVATE_IP`
  * `testPrivateKey` ↔ `TEST_PRIVATE_KEY`
  * `testPublicKey` ↔ `TEST_PUBLIC_KEY`
  * `testApiKey` ↔ `TEST_BEARER_TOKEN`
  * `remoteUsername` ↔ `REMOTE_USER_NAME`
  * `orchestratorSshKeyName` ↔ `ORCHESTRATOR_SSH_KEY_FILE_NAME`
  * `orchestratorPublicKey` ↔ `ORCHESTRATOR_PUBLIC_KEY`
  * `orchestratorInterfaceName` ↔ `ORCHESTRATOR_INTERFACE_NAME`
  * `orchestratorPrivateIp` ↔ `ORCHESTRATOR_PRIVATE_IP`
  * `orchestratorPublicIp` ↔ `ORCHESTRATOR_PUBLIC_IP`
  * `orchestratorListeningPort` ↔ `ORCHESTRATOR_WG_PORT`
  * `orchestratorPort` ↔ `ORCHESTRATOR_PORT`
  * `chelseaSshKeyName` ↔ `CHELSEA_SSH_KEY_FILE_NAME`
  * `chelseaPublicKey` ↔ `CHELSEA_PUBLIC_KEY`
  * `chelseaInterfaceName` ↔ `CHELSEA_INTERFACE_NAME`
  * `chelseaPrivateIp` ↔ `CHELSEA_PRIVATE_IP`
  * `chelseaPublicIp` ↔ `CHELSEA_PUBLIC_IP`
  * `chelseaListeningPort` ↔ `CHELSEA_WG_PORT`
  * `chelseaPort` ↔ `CHELSEA_SERVER_PORT`
  * `proxyPublicIp` ↔ `PROXY_PUBLIC_IP`
  * `proxyHost` ↔ `PROXY_HOST`
  * `proxyPort` ↔ `PROXY_PORT`

### `loadEnv channel overwrite`

* **Purpose:** Load all mapped environment variables into namespace variables.
* **Params:**
  * `channel` — output channel for warning messages.
  * `overwrite` — if `true`, overwrite existing variables.
* **Raises:** if required environment variables were missing or invalid.
* **Notes:** Calls `setupEnvVarMappings` and then loads each variable.

---

## .env File Management

### `maybeCopyDotEnvFile sourceDirectory targetDirectory overwrite withRoot`

* **Purpose:** Copy a `.env` file from source to target directory if it exists.
* **Params:**
  * `sourceDirectory` — directory containing source `.env` file.
  * `targetDirectory` — destination directory.
  * `overwrite` — if `true`, overwrite existing target file.
  * `withRoot` — if `true`, use `sudo` for the copy operation.
* **Raises:** if source or target directory doesn't exist.
* **Notes:** Does nothing if source `.env` doesn't exist or target exists (when not overwriting).

### `maybeWriteVariableToDotEnvFile fileName varName varValue overwrite channel verbose quiet`

* **Purpose:** Add or update a variable in a `.env` file.
* **Params:**
  * `fileName` — path to the `.env` file.
  * `varName` — environment variable name (must be alphanumeric/underscore).
  * `varValue` — value to set.
  * `overwrite` — if `true`, update existing variable; if `false`, skip if present.
  * `channel` — output channel for messages.
  * `verbose` — if `true`, use `tputs`; otherwise use `tlog`.
  * `quiet` — if `true`, suppress all output.
* **Returns:** `true` if variable was written, `false` if skipped.
* **Raises:** if `varName` is not alphanumeric.
* **Example:**

  ```tcl
  ::Chelsea::maybeWriteVariableToDotEnvFile /tmp/.env NODE_ID abc123
  ```

---

## Dependencies & Environment

* **Eagle packages:** `Eagle.Test` (for `tputs`, `tlog`).
* **Other Chelsea helpers:** `buildExecCommand`, `evalExecCommand`, `randomHexInteger`.
* **External tools:** `cp` (when `withRoot` is true).

---

*Package:* `Chelsea.Configuration 1.0` · *Namespace:* `::Chelsea`
