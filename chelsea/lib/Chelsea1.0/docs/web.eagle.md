# Chelsea Web Helpers

Procedures in the `::Chelsea` namespace for HTTP requests via cURL. Provides URI building and a comprehensive cURL wrapper for API interactions.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## URI Building

### `buildFullUri baseUri query`

* **Purpose:** Build a complete URI with query parameters.
* **Params:**
  * `baseUri` — the base URI string.
  * `query` — flat list of name/value pairs for query parameters.
* **Returns:** Full URI with properly escaped query string.
* **Notes:**
  * Adds `?` for first parameter, `&` for subsequent.
  * Uses `uri escape data` for URL encoding.
* **Example:**

  ```tcl
  set uri [::Chelsea::buildFullUri "https://api.example.com/v1/vms" \
      [list page 1 limit 10 status "running"]]
  # "https://api.example.com/v1/vms?page=1&limit=10&status=running"
  ```

---

## HTTP Requests

### `execCurlCommand baseUri host apiKey method path data query`

* **Purpose:** Execute an HTTP request using cURL.
* **Params:**
  * `baseUri` — base URI for the request (required, must be valid URI).
  * `host` — optional Host header value.
  * `apiKey` — API key for Bearer authentication (use `"none"` to disable auth).
  * `method` — HTTP method (default: `GET`). Must be valid HTTP verb.
  * `path` — path to append to base URI.
  * `data` — JSON body for POST/PUT requests.
  * `query` — query parameters as a flat list.
* **Returns:** Dictionary with:
  * `exitCode` — cURL exit code.
  * `responseCode` — HTTP status code (e.g., `200`, `404`).
  * `stdOut` — response body (trimmed).
  * `stdErr` — error output (trimmed).
* **Raises:**
  * if `baseUri` is not a valid URI.
  * if `method` is not a valid HTTP verb.
  * if combined URI is not valid.
  * if query parameters are not a valid list.
  * if full URI contains quotes or is invalid.
  * if API key is required but invalid.
  * if HTTP response output is malformed.
  * if HTTP response code is invalid.
* **cURL Options Used:**
  * `-s` — silent mode.
  * `-S` — show errors.
  * `-w " %{http_code}"` — append HTTP status code.
  * `-X <method>` — HTTP method.
  * `-H "Host: ..."` — Host header (if provided).
  * `-H "Authorization: Bearer ..."` — auth header (if API key provided).
  * `-H "Content-Type: application/json"` — content type (if data provided).
  * `-d @<file>` — request body from temp file.
* **Notes:**
  * Temporary file created for request body is deleted if cleanup is enabled.
  * Response code is extracted from the last 3 characters of output.
  * Uses `extractTestApiKey` when `apiKey` is empty but authorization is needed.
* **Example:**

  ```tcl
  # Simple GET request
  set r [::Chelsea::execCurlCommand "https://api.example.com" "" "" GET "/status"]
  puts "Status: [dict get $r responseCode]"
  puts "Body: [dict get $r stdOut]"

  # POST with JSON body
  set r [::Chelsea::execCurlCommand "https://api.example.com" "" $apiKey POST \
      "/vms" {{"name": "test-vm"}}]

  # GET with query parameters
  set r [::Chelsea::execCurlCommand "https://api.example.com" "" $apiKey GET \
      "/vms" "" [list status running limit 10]]
  ```

---

## Authentication Modes

| `apiKey` Value | Behavior |
|----------------|----------|
| `"none"` | No Authorization header sent |
| `""` (empty) | Uses `extractTestApiKey` to get key |
| Valid key | Uses provided key for Bearer auth |

---

## Response Format

The returned dictionary always contains:

```tcl
{
  exitCode    <cURL exit status>
  responseCode <HTTP status code, e.g., "200">
  stdOut      <response body, trimmed>
  stdErr      <error output, trimmed>
}
```

Or on execution error:

```tcl
{error <error message>}
```

---

## Typical Usage

**Basic API call:**

```tcl
set result [::Chelsea::execCurlCommand $baseUri "" $apiKey GET "/api/version"]

if {[dict get $result responseCode] eq "200"} {
  set body [dict get $result stdOut]
  puts "Response: $body"
}
```

**POST request with data:**

```tcl
set json {{"name": "my-vm", "memory": 1024}}
set result [::Chelsea::execCurlCommand $baseUri "" $apiKey POST "/api/vms" $json]

if {[dict get $result responseCode] eq "201"} {
  puts "VM created!"
}
```

**Request with custom Host header:**

```tcl
set result [::Chelsea::execCurlCommand $proxyUri "api.internal.com" $apiKey GET "/status"]
```

---

## Dependencies & Environment

* **Other Chelsea helpers:** `getHttpVerbs`, `isValidApiKeyId`, `isValidHttpResponseCode`, `buildExecCommand`, `evalInDirectory`, `formatExecResults`, `getDictionaryValue`, `isCleanupEnabled`, `getTemporaryPath`, `extractTestApiKey`.
* **External tools:** `curl`.
* **Eagle commands:** `uri escape`, `string is uri`.

---

*Package:* `Chelsea.Web 1.0` · *Namespace:* `::Chelsea`
