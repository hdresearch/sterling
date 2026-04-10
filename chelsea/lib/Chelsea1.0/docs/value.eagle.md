# Chelsea Value Helpers

Procedures in the `::Chelsea` namespace for value generation, validation, and string manipulation. Provides utilities for working with identifiers, ports, HTTP codes, and various data formats.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).

---

## Random Value Generation

### `randomHexInteger`

* **Purpose:** Generate a random 64-bit hex string.
* **Returns:** 16-character lowercase hex string (e.g., `a1b2c3d4e5f60718`).
* **Notes:**
  * Can use simulated randomness via `chelsea_random_numbers` and `chelsea_random_index` variables.
  * Set `::no(simulatedRandomness)` to disable simulated mode.
* **Example:**

  ```tcl
  set hex [::Chelsea::randomHexInteger]
  # "7f3a2b1c8d9e0f12"
  ```

### `randomId`

* **Purpose:** Generate a random GUID-formatted identifier.
* **Returns:** GUID string (e.g., `a1b2c3d4-e5f6-0718-9abc-def012345678`).
* **Notes:**
  * Uses `guid new` if `::no(simulatedRandomness)` is set.
  * Otherwise constructs from two `randomHexInteger` calls.
* **Example:**

  ```tcl
  set id [::Chelsea::randomId]
  # "7f3a2b1c-8d9e-0f12-3456-789abcdef012"
  ```

---

## Validation Functions

### `isValidPort value`

* **Purpose:** Validate a TCP/UDP port number.
* **Returns:** `true` if valid (integer 0-65535), `false` otherwise.
* **Example:**

  ```tcl
  ::Chelsea::isValidPort 8080   ;# true
  ::Chelsea::isValidPort 70000  ;# false
  ::Chelsea::isValidPort "abc"  ;# false
  ```

### `isValidClusterId value`

* **Purpose:** Validate a cluster ID (Flickr Base58 format).
* **Returns:** `true` if valid (22-character Base58 string), `false` otherwise.
* **Notes:** Uses flickrBase58 alphabet: `123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ` (no 0, O, I, l).
* **Example:**

  ```tcl
  ::Chelsea::isValidClusterId "abc123DEF456ghi789jkLM"  ;# true (22 chars)
  ```

### `isValidId value`

* **Purpose:** Validate a GUID string.
* **Returns:** `true` if valid GUID format, `false` otherwise.
* **Example:**

  ```tcl
  ::Chelsea::isValidId "a1b2c3d4-e5f6-7890-abcd-ef1234567890"  ;# true
  ```

### `isValidVirtualMachineId value`

* **Purpose:** Validate a virtual machine identifier.
* **Returns:** Result of `isValidId` (GUID validation).

### `isValidCommitId value`

* **Purpose:** Validate a commit identifier.
* **Returns:** Result of `isValidId` (GUID validation).

### `isValidApiKeyId value`

* **Purpose:** Validate an API key (GUID + 64 hex characters).
* **Returns:** `true` if format matches, `false` otherwise.
* **Notes:** Format is 36-character GUID followed by 64 hex characters.

### `isValidIdentifier value`

* **Purpose:** Validate an identifier (variable/function name format).
* **Returns:** `true` if matches `^[A-Z][_0-9A-Z]*$` (case-insensitive), `false` otherwise.

### `isValidHttpResponseCode value zeros`

* **Purpose:** Validate an HTTP response code.
* **Params:**
  * `value` — the code to validate.
  * `zeros` — if `true`, accept "000" as valid.
* **Returns:** `true` if valid (100-599, or "000" when zeros=true), `false` otherwise.

### `isValidFileNameOnly value`

* **Purpose:** Validate a file name (no path separators).
* **Returns:** `true` if matches `^[\-_0-9A-Za-z]+(?:\.[\-_0-9A-Za-z]+)?$`, `false` otherwise.
* **Example:**

  ```tcl
  ::Chelsea::isValidFileNameOnly "config.json"  ;# true
  ::Chelsea::isValidFileNameOnly "/etc/config"  ;# false
  ```

### `isValidTimeStamp value`

* **Purpose:** Validate a Unix timestamp.
* **Returns:** `true` if non-negative wide integer, `false` otherwise.

### `isValidOwner value`

* **Purpose:** Validate a file owner string.
* **Returns:** `true` if valid format, `false` otherwise.
* **Notes:**
  * **Windows:** SID format `S-1-5-...`
  * **POSIX:** `username` or `username:group` with alphanumeric/hyphen characters.

### `isValidPermissions value`

* **Purpose:** Validate file permissions (POSIX only).
* **Returns:** `true` if integer 0-0o777, `false` otherwise (or on Windows).

---

## HTTP Validation

### `verifyHttpResponseCode response wantResponseCode`

* **Purpose:** Verify an HTTP response code matches expected value.
* **Params:**
  * `response` — dictionary containing `responseCode`.
  * `wantResponseCode` — expected code (default: 200).
* **Raises:**
  * if response code is invalid.
  * if response code doesn't match expected.

### `getHttpVerbs`

* **Purpose:** Get list of valid HTTP verbs.
* **Returns:** `{CONNECT DELETE GET HEAD OPTIONS PATCH POST PUT TRACE}`.

---

## String Utilities

### `normalizeSpaces value`

* **Purpose:** Replace multiple whitespace characters with a single space.
* **Returns:** Normalized string.
* **Example:**

  ```tcl
  ::Chelsea::normalizeSpaces "hello   world\n\tfoo"
  # "hello world foo"
  ```

### `extractIdentifiers value`

* **Purpose:** Extract 32-character hex identifiers from a string.
* **Returns:** List of matched identifiers.

### `stringIsSshPrivateKey value`

* **Purpose:** Check if a string is a valid OpenSSH private key.
* **Returns:** `true` if matches OpenSSH private key format, `false` otherwise.
* **Notes:** Validates header, base64 content, and footer markers.

### `globForHexDigits count`

* **Purpose:** Generate a glob pattern for hex digits.
* **Params:**
  * `count` — number of hex digits.
* **Returns:** Pattern string like `[0-9a-f][0-9a-f]...`.

### `globForUniqueIdentifier`

* **Purpose:** Generate a glob pattern for GUID format.
* **Returns:** Pattern like `[0-9a-f][0-9a-f]...-[0-9a-f]...-...`.

---

## Path Utilities

### `getDirectoryOnly value`

* **Purpose:** Extract the directory portion of a path.
* **Params:**
  * `value` — absolute path.
* **Returns:** Directory path.
* **Raises:** if value is not a valid absolute path.
* **Notes:** Returns the path itself if it's a directory, or parent if it's a file.

### `makeRelativePath value`

* **Purpose:** Convert an absolute path to a relative path by stripping the leading slash.
* **Params:**
  * `value` — absolute path starting with `/`.
* **Returns:** Path without leading slash.
* **Raises:** if path doesn't start with `/`.
* **Example:**

  ```tcl
  ::Chelsea::makeRelativePath "/etc/config"
  # "etc/config"
  ```

---

## Dependencies

* **Eagle commands:** `string is`, `regexp`, `expr`, `guid new`, `isWindows`.

---

*Package:* `Chelsea.Value 1.0` · *Namespace:* `::Chelsea`
