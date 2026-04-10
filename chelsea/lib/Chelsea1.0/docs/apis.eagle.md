# Chelsea API Client Helpers

Procedures in the `::Chelsea` namespace for interacting with the Chelsea service APIs. Provides wrappers for public, internal, and proxy API endpoints with automatic response validation and JSON parsing.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value (often JSON or identifiers).
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## Public API

### `public_version proxy`

* **Purpose:** Get the service version from the public API.
* **Params:**
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** JSON string containing version information.
* **Raises:** on invalid response code or malformed JSON.

### `public_sshKey id proxy`

* **Purpose:** Retrieve SSH connection details for a VM from the public API.
* **Params:**
  * `id` — the virtual machine identifier.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** Dictionary with `port` (SSH port) and `privateKey` (OpenSSH private key).
* **Raises:** on invalid response, bad port number, or invalid private key format.

### `public_newVm json proxy`

* **Purpose:** Create a new virtual machine via the public API.
* **Params:**
  * `json` — JSON payload for VM creation.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** The new VM identifier (GUID).
* **Raises:** on HTTP status != 201, invalid JSON, or invalid VM ID.
* **Notes:** Automatically includes `wait_boot=true` parameter.

### `public_commitVm id proxy`

* **Purpose:** Commit a virtual machine state via the public API.
* **Params:**
  * `id` — the virtual machine identifier.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** The commit identifier (GUID).
* **Raises:** on HTTP status != 201, invalid JSON, or invalid commit ID.

### `public_resumeVm id json proxy`

* **Purpose:** Resume a virtual machine from a commit via the public API.
* **Params:**
  * `id` — the commit identifier to resume from.
  * `json` — JSON payload for resume options.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** The new VM identifier (GUID).
* **Raises:** on HTTP status != 201, invalid JSON, or invalid VM ID.

### `public_branchVm id json proxy`

* **Purpose:** Branch a virtual machine via the public API.
* **Params:**
  * `id` — the source virtual machine identifier.
  * `json` — JSON payload for branch options.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** The new VM identifier (GUID).
* **Raises:** on invalid response, JSON, or VM ID.

### `public_branchByVm id json proxy`

* **Purpose:** Branch by virtual machine and return multiple VMs via the public API.
* **Params:**
  * `id` — the source virtual machine identifier.
  * `json` — JSON payload for branch options.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** The first VM identifier from the `vms` array (GUID).
* **Raises:** on HTTP status != 201, invalid JSON, or invalid VM ID.

### `public_branchByCommit id json proxy`

* **Purpose:** Branch by commit identifier via the public API.
* **Params:**
  * `id` — the commit identifier to branch from.
  * `json` — JSON payload for branch options.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** The first VM identifier from the `vms` array (GUID).
* **Raises:** on HTTP status != 201, invalid JSON, or invalid VM ID.

### `public_deleteVm id proxy`

* **Purpose:** Delete a virtual machine via the public API.
* **Params:**
  * `id` — the virtual machine identifier.
  * `proxy` — boolean indicating whether to route through the proxy.
* **Returns:** The deleted VM identifier.
* **Raises:** on invalid HTTP response code.

### `public_listCommits proxy {limit ""} {offset ""}`

* **Purpose:** List commits owned by the calling API key.
* **Params:**
  * `proxy` — whether to route through the proxy.
  * `limit` — optional page size (defaults server-side).
  * `offset` — optional result offset.
* **Returns:** JSON string with `commits`, `total`, `limit`, and `offset`.
* **Raises:** on invalid HTTP response code or malformed JSON.

### `public_deleteCommit id proxy`

* **Purpose:** Delete a commit owned by the calling API key.
* **Params:**
  * `id` — the commit identifier.
  * `proxy` — whether to route through the proxy.
* **Returns:** The deleted commit identifier.
* **Raises:** on invalid HTTP response code (e.g., forbidden, conflict).

---

## Internal API

### `internal_version`

* **Purpose:** Get the service version from the internal API.
* **Returns:** JSON string containing version information.
* **Raises:** on invalid response code or malformed JSON.

### `internal_listVm`

* **Purpose:** List all virtual machines via the internal API.
* **Returns:** List of VM identifiers (GUIDs).
* **Raises:** on invalid response, JSON, or any invalid VM ID in the list.

### `internal_network id`

* **Purpose:** Get network information for a VM via the internal API.
* **Params:**
  * `id` — the virtual machine identifier.
* **Returns:** JSON string containing network details.
* **Raises:** on invalid response code or malformed JSON.

### `internal_sshKey id`

* **Purpose:** Retrieve SSH connection details for a VM from the internal API.
* **Params:**
  * `id` — the virtual machine identifier.
* **Returns:** Dictionary with `port` (SSH port) and `privateKey` (OpenSSH private key).
* **Raises:** on invalid response, bad port number, or invalid private key format.

### `internal_newVm json`

* **Purpose:** Create a new virtual machine via the internal API.
* **Params:**
  * `json` — JSON payload for VM creation.
* **Returns:** The new VM identifier (GUID).
* **Raises:** on invalid response, JSON, or VM ID.
* **Notes:** Automatically includes `wait_boot=true` parameter.

### `internal_commitVm id json`

* **Purpose:** Commit a virtual machine state via the internal API.
* **Params:**
  * `id` — the virtual machine identifier.
  * `json` — JSON payload for commit options.
* **Returns:** The commit identifier (GUID).
* **Raises:** on invalid response, JSON, or commit ID.

### `internal_resumeVm id json`

* **Purpose:** Resume a virtual machine from a commit via the internal API.
* **Params:**
  * `id` — the commit identifier.
  * `json` — JSON payload for resume options.
* **Returns:** The new VM identifier (GUID).
* **Raises:** on invalid response, JSON, or VM ID.

### `internal_branchVm id json`

* **Purpose:** Branch a virtual machine via the internal API.
* **Params:**
  * `id` — the source virtual machine identifier.
  * `json` — JSON payload containing `vm_id`.
* **Returns:** The VM identifier from the JSON payload.
* **Raises:** on invalid response or VM ID.

### `internal_deleteVm id`

* **Purpose:** Delete a virtual machine via the internal API.
* **Params:**
  * `id` — the virtual machine identifier.
* **Returns:** The deleted VM identifier.
* **Raises:** on invalid response code or non-empty response body.

### `internal_health`

* **Purpose:** Check service health via the internal API.
* **Returns:** Response body string (typically empty on success).
* **Raises:** on invalid HTTP response code.

### `internal_telemetry`

* **Purpose:** Get telemetry data via the internal API.
* **Returns:** JSON string containing telemetry information.
* **Raises:** on invalid response code or malformed JSON.

### `internal_sleepVm id`

* **Purpose:** Put a virtual machine to sleep via the internal API.
* **Params:**
  * `id` — the virtual machine identifier.
* **Returns:** The VM identifier.
* **Raises:** on invalid HTTP response code or non-empty response body.

### `internal_wakeVm id json`

* **Purpose:** Wake a sleeping virtual machine via the internal API.
* **Params:**
  * `id` — the virtual machine identifier.
  * `json` — JSON payload for wake options.
* **Returns:** The VM identifier.
* **Raises:** on invalid HTTP response code or non-empty response body.

### `internal_getVmState id`

* **Purpose:** Get the current state of a virtual machine via the internal API.
* **Params:**
  * `id` — the virtual machine identifier.
* **Returns:** The VM state string (alphabetic, e.g., `running`, `sleeping`).
* **Raises:** on invalid response code, malformed JSON, or non-alphabetic state value.

---

## Proxy API

### `proxy_version`

* **Purpose:** Get the service version from the proxy API.
* **Returns:** JSON string containing version information.
* **Raises:** on invalid response code or malformed JSON.
* **Notes:** Always routes through proxy (`proxy true`).

---

## Dependencies & Environment

* **Other Chelsea helpers:** `makeApiCall`, `verifyHttpResponseCode`, `getDictionaryValue`, `isValidJson`, `jqRawString`, `isValidPort`, `stringIsSshPrivateKey`, `isValidVirtualMachineId`, `isValidCommitId`.
* **Global variables:** `proxyApiHost`, `publicApiHost`, `internalApiHost`, `headerHost`.

---

*Package:* `Chelsea.Apis 1.0` · *Namespace:* `::Chelsea`
