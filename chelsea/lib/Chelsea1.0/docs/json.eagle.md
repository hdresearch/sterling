# Chelsea JSON Helpers

Procedures in the `::Chelsea` namespace for JSON processing using the `jq` command-line tool. Provides wrappers for querying JSON data and converting JSON arrays to Tcl lists.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## Core jq Wrapper

### `jqCore json query raw`

* **Purpose:** Execute a `jq` query against JSON data.
* **Params:**
  * `json` — the JSON string to process.
  * `query` — the jq query expression (e.g., `.data.id`, `.vms[]`).
  * `raw` — if `true`, use `-r` flag for raw string output (no quotes).
* **Returns:** The query result from jq's stdout.
* **Raises:**
  * if jq command fails (non-Success exit code).
  * on any execution error.
* **Notes:**
  * Writes JSON to a temporary file for processing.
  * Temporary file is deleted if `isCleanupEnabled` returns true.
* **Example:**

  ```tcl
  set result [::Chelsea::jqCore $json ".data.status" true]
  ```

---

## Query Functions

### `jq json query`

* **Purpose:** Execute a jq query and return the result with JSON formatting preserved.
* **Params:**
  * `json` — the JSON string to process.
  * `query` — the jq query expression.
* **Returns:** The query result as JSON (strings include quotes).
* **Example:**

  ```tcl
  set result [::Chelsea::jq $json ".data"]
  # Returns: {"status": "ok", "id": "123"}
  ```

### `jqRawString json query`

* **Purpose:** Execute a jq query and return the raw string value (no JSON quotes).
* **Params:**
  * `json` — the JSON string to process.
  * `query` — the jq query expression.
* **Returns:** The raw string value without JSON encoding.
* **Example:**

  ```tcl
  set status [::Chelsea::jqRawString $json ".data.status"]
  # Returns: ok (not "ok")

  set id [::Chelsea::jqRawString $json ".vm_id"]
  # Returns: abc-123-def
  ```

---

## Array Conversion

### `jsonArrayToList json`

* **Purpose:** Convert a JSON array into a Tcl list.
* **Params:**
  * `json` — JSON string containing an array.
* **Returns:** Tcl list of JSON elements.
* **Notes:**
  * Iterates through array indices (0, 1, 2, ...) until `null` is returned.
  * Each element is returned as a JSON string (use `jqRawString` for scalars).
* **Example:**

  ```tcl
  set json {["apple", "banana", "cherry"]}
  set fruits [::Chelsea::jsonArrayToList $json]
  # Returns: {"apple"} {"banana"} {"cherry"}

  # For complex objects:
  set json {[{"name": "vm1"}, {"name": "vm2"}]}
  set vms [::Chelsea::jsonArrayToList $json]
  foreach vm $vms {
    set name [::Chelsea::jqRawString $vm ".name"]
    puts "VM: $name"
  }
  ```

---

## Typical Usage

**Extract a single value:**

```tcl
set json {{"vm_id": "abc-123", "status": "running"}}
set vmId [::Chelsea::jqRawString $json ".vm_id"]
puts "VM ID: $vmId"
```

**Navigate nested structures:**

```tcl
set json {{"data": {"vms": [{"id": "vm1"}, {"id": "vm2"}]}}}
set firstVmId [::Chelsea::jqRawString $json ".data.vms[0].id"]
puts "First VM: $firstVmId"
```

**Process arrays:**

```tcl
set json {{"vms": [{"id": "vm1"}, {"id": "vm2"}]}}
set vmIds [::Chelsea::jqRawString $json {.vms[].id}]
# Returns: vm1\nvm2 (newline-separated)
```

---

## Dependencies & Environment

* **Other Chelsea helpers:** `buildExecCommand`, `evalExecCommand`, `formatExecResults`, `getDictionaryValue`, `isCleanupEnabled`.
* **External tools:** `jq` (command-line JSON processor).
* **Notes:** Requires `jq` to be installed and available in PATH.

---

*Package:* `Chelsea.Json 1.0` · *Namespace:* `::Chelsea`
