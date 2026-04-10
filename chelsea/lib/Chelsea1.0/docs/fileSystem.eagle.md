# Chelsea File-System Helpers

Procedures in the `::Chelsea` namespace for packaging files into an SQLite database, token expansion, manifest processing, and deploying files onto a target filesystem. Designed for Eagle test harnesses and compatible with Tcl evaluation.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value (often `""` for success).
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.
> * **Examples** use Eagle/Tcl snippets.

---

## SQL Builders

### `getSelectSql`

* **Purpose:** Get SQL query to read all rows from the `Files` table.
* **Returns:** SQL string with columns: `Id`, `Sequence`, `Description`, `Type`, `TargetId`, `Path`, `Name`, `Owner`, `Permissions`, `Modified`, `Content`, `HashAlgorithm`, `SignatureAlgorithm`, `PublicKeyToken`, `Signature`.
* **Notes:** Results are ordered by `Sequence ASC`.
* **Example:**

  ```tcl
  set sql [::Chelsea::getSelectSql]
  sql execute -execute reader -format array -- $conn $sql
  ```

### `getInsertSql`

* **Purpose:** Get parameterized SQL to insert one row into the `Files` table.
* **Returns:** SQL string with 7 parameters (`?` placeholders).
* **Parameters (in order):**
  1. `Path` — relative directory path
  2. `Name` — file base name
  3. `Owner` — owner string (e.g., `root:root`)
  4. `Permissions` — octal mode integer
  5. `Modified` — modification timestamp (epoch seconds)
  6. `Content` — file content (binary)
  7. `Signature` — GPG signature (binary)
* **Notes:**
  * `Id` is auto-generated as `RANDOMBLOB(16)`.
  * `Sequence` auto-increments from existing max.

---

## File Name Validation

### `getAndCheckFileName directory row index`

* **Purpose:** Build and validate an absolute target filename from a database row.
* **Params:**
  * `directory` — root directory for deployment.
  * `row` — database row with `Path` and `Name` columns.
  * `index` — row index for error messages.
* **Returns:** Absolute path where the file will be created.
* **Raises:**
  * if `Name` is not a valid base filename.
  * if constructed path is not valid or absolute.
  * if path contains NUL, space, backslash, `/./ `, or `/../`.
  * if file already exists at the path.
* **Notes:** Uses `resolveTokens` with `viaTokenFile=true` for path expansion.

---

## Database Building

### `buildDeploymentDatabase path fileName directory`

* **Purpose:** Create an SQLite database containing files for deployment.
* **Params:**
  * `path` — package path (for schema location).
  * `fileName` — output database file path.
  * `directory` — source directory for files.
* **Flow:**
  1. Open database in read-write mode.
  2. Execute schema from `<package>/data/schema/files.sql`.
  3. Get `keyId` and `passphraseFileName` from caller (upvar).
  4. Read manifest via `getFilesFromManifest`.
  5. Import files via `importFiles`.
  6. Close database.
* **Raises:** on database, schema, or import errors.
* **Example:**

  ```tcl
  ::Chelsea::maybeSetupForTestGpgSigning $root
  ::Chelsea::buildDeploymentDatabase $root $dbFile [file join $root data]
  ```

### `findToolDataFileName directory fileNameOnly`

* **Purpose:** Search for a tool data file by walking up the directory tree.
* **Params:**
  * `directory` — starting directory.
  * `fileNameOnly` — base name of file to find.
* **Returns:** Absolute path to the file.
* **Raises:** if file is not found up to the volume root.
* **Search Order:** For each directory level:
  1. `<dir>/<fileNameOnly>`
  2. `<dir>/tools/data/<fileNameOnly>`

---

## Deployment

### `deployToFileSystem databaseDirectory targetDirectory`

* **Purpose:** Deploy files from a database to the filesystem.
* **Params:**
  * `databaseDirectory` — directory to search for `fileSystem.db`.
  * `targetDirectory` — destination directory.
* **Flow:**
  1. Create temporary staging directory.
  2. Find `fileSystem.db` via `findToolDataFileName`.
  3. Open database (read-only) and load all rows.
  4. Deploy files to staging via `deployFiles`.
  5. Copy staging to target via `sudo cp -v -r --update=none --no-dereference --preserve=all --no-preserve=context,xattr --one-file-system`.
  6. Close database and delete temporary directory.
* **Raises:** on database, copy, or deployment errors.

### `deployFiles directory varName quiet channel`

* **Purpose:** Materialize database rows as actual files.
* **Params:**
  * `directory` — target directory for file creation.
  * `varName` — variable name containing rows array (default: `rows`).
  * `quiet` — if `true`, suppress output.
  * `channel` — output channel for logging.
* **Process Per Row:**
  1. Validate `Id` as GUID.
  2. Resolve path via `getAndCheckFileName`.
  3. Create parent directories.
  4. Handle by `Type`:
     * **Empty (`""`)**: Write content (verify signature if present), log size and SHA1.
     * **`SymbolicLink`**: Find target row by `TargetId`, create symlink.
     * **Other**: Error.
  5. Set modification time from `Modified` (if present).
  6. Set owner from `Owner` (if present).
  7. Set permissions from `Permissions` (if present).
* **Raises:** on invalid inputs, type errors, or execution failures.
* **Example:**

  ```tcl
  ::Chelsea::openDatabase $db true conn
  sql execute -execute reader -format array -- $conn [::Chelsea::getSelectSql]
  ::Chelsea::deployFiles /tmp/staging rows
  ::Chelsea::closeDatabase conn
  ```

---

## Token Files & Expansion

### `getTokensFileName`

* **Purpose:** Get the path to the token mapping file.
* **Returns:** `<package>/data/fileSystem/token.lst`.

### `parseTokens fileName`

* **Purpose:** Parse a token mapping file.
* **Params:**
  * `fileName` — path to token file.
* **Returns:** Flat list of name/value pairs: `name1 value1 name2 value2 ...`.
* **File Format:** Tab-separated `NAME<TAB>VALUE`, `#` comments allowed.

### `resolveTokens value viaTokenFile`

* **Purpose:** Expand token placeholders in a string.
* **Params:**
  * `value` — string with `%NAME%` placeholders.
  * `viaTokenFile` — if `true`, use token file; otherwise expand `%kernel%` via `uname -r`.
* **Returns:** Expanded string.
* **Example:**

  ```tcl
  set path [::Chelsea::resolveTokens "/lib/modules/%kernel%/kernel" false]
  # "/lib/modules/5.15.0-generic/kernel"
  ```

---

## Manifests & Import

### `getManifestFileName directory`

* **Purpose:** Get the path to the file manifest.
* **Params:**
  * `directory` — base directory.
* **Returns:** `$directory/data/fileSystem.lst`.

### `getFilesFromManifest fileName directory`

* **Purpose:** Read and validate files listed in a manifest.
* **Params:**
  * `fileName` — manifest file path.
  * `directory` — base directory for resolving files.
* **Returns:** List of original (unexpanded) relative file paths.
* **Raises:**
  * if manifest file doesn't exist.
  * if directory doesn't exist.
  * if any expanded file path doesn't exist.
* **Notes:** Ignores empty lines and `#` comments.

### `importFiles directory fileNames connection keyId passphraseFileName`

* **Purpose:** Import files into the database with signatures.
* **Params:**
  * `directory` — source directory.
  * `fileNames` — list of relative file paths.
  * `connection` — database connection.
  * `keyId` — optional GPG key ID.
  * `passphraseFileName` — optional GPG passphrase file.
* **For Each File:**
  1. Resolve tokens and build local path.
  2. Create signature if `.asc` file doesn't exist.
  3. Insert into database with:
     * `Path`: relative directory
     * `Name`: file basename
     * `Owner`: `root:root`
     * `Permissions`: `0o644`
     * `Modified`: file mtime
     * `Content`: file bytes
     * `Signature`: signature bytes
* **Raises:** on I/O, signing, or database errors.

---

## Quick Cookbook

**Create database from manifest and deploy:**

```tcl
set root /opt/mytool
set db   /tmp/fileSystem.db

::Chelsea::maybeSetupForTestGpgSigning $root
::Chelsea::buildDeploymentDatabase $root $db [file join $root data]
::Chelsea::deployToFileSystem [file dirname $db] /var/lib/chelsea
::Chelsea::maybeCleanupForTestGpgSigning
```

**Read and deploy manually:**

```tcl
::Chelsea::openDatabase $db true conn
sql execute -execute reader -format array -datetimebehavior seconds \
    -blobbehavior object -alias -- $conn [::Chelsea::getSelectSql]

set staging "/tmp/deploy"
file mkdir $staging
::Chelsea::deployFiles $staging rows

::Chelsea::closeDatabase conn
```

---

## Dependencies & Environment

* **Other Chelsea helpers:**
  * From `Chelsea.Database`: `openDatabase`, `closeDatabase`, `getColumnValueEx`, `findRow`, `readFileBytes`, `writeFileBytes`.
  * From `Chelsea.Gpg`: `signFileWithGpg`, `maybeVerifySignatureWithGpg`.
  * From `Chelsea.Value`: `isValidFileNameOnly`, `isValidTimeStamp`, `isValidOwner`, `isValidPermissions`, `makeRelativePath`.
  * From `Chelsea.Shell`: `buildExecCommand`, `evalExecCommand`.
  * From `Chelsea.TemporaryDirectory`: `maybeCreateTemporaryDirectory`, `maybeDeleteTemporaryDirectory`, `verifyTemporaryDirectory`.
  * From `Chelsea.Configuration`: `getPackageDirectory`.
* **Eagle packages/commands:** `sql`, `hash`, `tputs`.
* **External tools:** `cp`, `chmod`, `chown`, `ln`, `uname`.
* **Runtime:** Linux/POSIX paths assumed.

---

*Package:* `Chelsea.FileSystem 1.0` · *Namespace:* `::Chelsea`
