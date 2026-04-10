# Chelsea Database Helpers

Procedures in the `::Chelsea` namespace for SQLite database operations, including connection management, row searching, and binary file I/O. Provides the foundation for data persistence across the Chelsea test suite.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## Database Path

### `getServiceDatabaseFileName`

* **Purpose:** Get the path to the service database file.
* **Returns:** Value of `chelsea_database_path` if set, otherwise `/var/lib/chelsea/db/chelsea.db`.
* **Example:**

  ```tcl
  set dbFile [::Chelsea::getServiceDatabaseFileName]
  ```

---

## Connection Management

### `getConnectionString readOnly`

* **Purpose:** Build an ADO.NET connection string for SQLite.
* **Params:**
  * `readOnly` — if `true`, add `Read Only=true;` to the connection string.
* **Returns:** Connection string with `Data Source`, `DateTimeKind=Utc`, and `DateTimeFormat=UnixEpoch`.
* **Example:**

  ```tcl
  set connStr [::Chelsea::getConnectionString false]
  # "Data Source=${fileName};DateTimeKind=Utc;DateTimeFormat=UnixEpoch;"
  ```

### `getConnectionObject varName`

* **Purpose:** Get the underlying connection object handle from an open connection.
* **Params:**
  * `varName` — name of the variable containing the connection identifier.
* **Returns:** The connection object handle.
* **Raises:** if the connection variable doesn't exist or the object lookup fails.
* **Notes:** Uses Eagle's interpreter introspection to retrieve the object.

### `getSQLiteBinaryFileName`

* **Purpose:** Get the path to the SQLite native interop library.
* **Returns:** Path to `SQLite.Interop.dll` in the same directory as the shell executable.

### `openDatabase fileName readOnly varName`

* **Purpose:** Open a SQLite database connection with regexp extension enabled.
* **Params:**
  * `fileName` — path to the database file.
  * `readOnly` — if `true`, open in read-only mode.
  * `varName` — variable name to store the connection (default: `connection`).
* **Returns:** Empty string on success.
* **Notes:**
  * Enables SQLite extensions.
  * Loads the `sqlite3_regexp_init` extension for regexp support.
  * Connection is stored in caller's scope via upvar.
* **Example:**

  ```tcl
  ::Chelsea::openDatabase /path/to/db.sqlite true conn
  # $conn now contains the connection identifier
  ```

### `closeDatabase varName`

* **Purpose:** Close a database connection and clean up resources.
* **Params:**
  * `varName` — variable name containing the connection (default: `connection`).
* **Returns:** Empty string.
* **Notes:** Disposes the object, closes the SQL connection, and unsets the variable.
* **Example:**

  ```tcl
  ::Chelsea::closeDatabase conn
  ```

---

## Row Operations

### `findRow columnNames columnValues rowsVarName notFound onError`

* **Purpose:** Search for a row matching specific column values in a result set.
* **Params:**
  * `columnNames` — list of column names to match.
  * `columnValues` — list of values to match (same order as column names).
  * `rowsVarName` — variable name containing the rows array (default: `rows`).
  * `notFound` — value to return if no match is found (default: `-1`).
  * `onError` — value to return on error (default: same as `notFound`).
* **Returns:** Row index (1-based) if found, `notFound` otherwise.
* **Notes:** Rows array must have been populated by `sql execute -format array`.
* **Example:**

  ```tcl
  set idx [::Chelsea::findRow {Id} {abc-123-def} rows]
  if {$idx != -1} {
    set matchedRow $rows($idx)
  }
  ```

### `getColumnValueEx row column default wrap`

* **Purpose:** Get a column value from a row with automatic GUID conversion.
* **Params:**
  * `row` — the row data from the result set.
  * `column` — column name to retrieve.
  * `default` — value to return if column is missing.
  * `wrap` — wrapper string (unused in current implementation).
* **Returns:** Column value, or GUID string if the value is a 16-byte blob.
* **Notes:** Automatically converts 16-byte `System#Byte[]` objects to GUID strings.
* **Example:**

  ```tcl
  set id [::Chelsea::getColumnValueEx $row Id]
  # Returns "abc12345-1234-5678-9abc-def012345678" for GUID blobs
  ```

---

## Binary File I/O

### `readFileAsBlob fileName`

* **Purpose:** Read a file and return its contents as an SQLite hex blob literal.
* **Params:**
  * `fileName` — path to the file to read.
* **Returns:** String in format `x'ABCD...'` suitable for SQL INSERT statements.
* **Example:**

  ```tcl
  set blob [::Chelsea::readFileAsBlob /path/to/file.bin]
  # "x'48656c6c6f...'"
  ```

### `readFileBytes fileName`

* **Purpose:** Read a file into a managed byte array object.
* **Params:**
  * `fileName` — path to the file to read.
* **Returns:** `System#Byte[]#<id>` object handle.
* **Notes:** The returned object must be disposed when no longer needed.
* **Example:**

  ```tcl
  set bytes [::Chelsea::readFileBytes /path/to/file.bin]
  puts "File size: [$bytes Length]"
  ```

### `writeFileBytes fileName bytes`

* **Purpose:** Write a managed byte array to a file.
* **Params:**
  * `fileName` — path to the output file.
  * `bytes` — `System#Byte[]` object handle.
* **Returns:** Empty (void operation).
* **Example:**

  ```tcl
  ::Chelsea::writeFileBytes /path/to/output.bin $bytes
  ```

---

## Typical Usage

**Open database, query, and close:**

```tcl
set dbFile [::Chelsea::getServiceDatabaseFileName]
::Chelsea::openDatabase $dbFile true conn

sql execute -execute reader -format array -- $conn {
  SELECT * FROM api_key WHERE rowId = 1;
}

set key [::Chelsea::getColumnValueEx $rows(1) id]
::Chelsea::closeDatabase conn
```

**Read and write binary files:**

```tcl
set content [::Chelsea::readFileBytes /path/to/source.bin]
::Chelsea::writeFileBytes /path/to/dest.bin $content
object dispose $content
```

---

## Dependencies & Environment

* **Eagle packages:** `sql` command, `object` command.
* **Other Chelsea helpers:** `getServiceDataDirectory`, `getShellExecutableName`, `haveColumnValue`, `getColumnValue`, `isNonNullObjectHandle`.
* **External:** SQLite.Interop.dll, System.Data.SQLite.dll.

---

*Package:* `Chelsea.Database 1.0` · *Namespace:* `::Chelsea`
