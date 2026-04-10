# Chelsea GPG Helpers

Procedures in the `::Chelsea` namespace for GPG (GNU Privacy Guard) signing and verification operations. Used for signing deployment files and verifying their integrity during the test suite.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## Test Setup/Cleanup

### `maybeSetupForTestGpgSigning path`

* **Purpose:** Set up default GPG signing parameters for testing if not already configured.
* **Params:**
  * `path` — base path for locating the passphrase file.
* **Side Effects:**
  * Sets `keyId` in caller's scope to default test key if not present.
  * Sets `passphraseFileName` in caller's scope to default passphrase file if not present.
  * Sets `keyIdSet` and `passphraseFileNameSet` flags in caller's scope.
  * Prints **WARNING** messages via `host result Continue`.
* **Defaults:**
  * `keyId = 8B7FF667057CDD53FFD840268C6DFB78B8AE0A57`
  * `passphraseFileName = $path/data/keys/passphrase.txt`
* **Notes:** Only sets variables if they don't already exist in the caller's scope.
* **Example:**

  ```tcl
  ::Chelsea::maybeSetupForTestGpgSigning /opt/chelsea
  # $keyId and $passphraseFileName now available in caller's scope
  ```

### `maybeCleanupForTestGpgSigning`

* **Purpose:** Clean up GPG signing parameters that were set by `maybeSetupForTestGpgSigning`.
* **Side Effects:**
  * Unsets `keyId` and `keyIdSet` if `keyIdSet` was set.
  * Unsets `passphraseFileName` and `passphraseFileNameSet` if `passphraseFileNameSet` was set.
  * Prints **WARNING** messages via `host result Continue`.
* **Example:**

  ```tcl
  ::Chelsea::maybeCleanupForTestGpgSigning
  ```

---

## Signing

### `signFileWithGpg dataFileName keyId passphraseFileName`

* **Purpose:** Create a detached ASCII-armored GPG signature for a file.
* **Params:**
  * `dataFileName` — path to the file to sign.
  * `keyId` — optional 40-character hex GPG key ID.
  * `passphraseFileName` — optional path to file containing the key passphrase.
* **Returns:** Result of `evalExecCommand` (execution result from `testExec`).
* **Raises:**
  * if `dataFileName` is not a valid file path or doesn't exist.
  * if `keyId` is provided but not a 40-character hex string.
  * if `passphraseFileName` is provided but doesn't exist.
* **Notes:**
  * Uses `gpg --batch --yes --detach-sign --armor`.
  * Uses `--pinentry-mode loopback` for passphrase input (GPG 2.1+).
  * Creates signature file at `<dataFileName>.asc`.
* **Example:**

  ```tcl
  ::Chelsea::signFileWithGpg /path/to/file.txt $keyId /path/to/passphrase.txt
  # Creates /path/to/file.txt.asc
  ```

---

## Verification

### `maybeVerifySignatureWithGpg row`

* **Purpose:** Verify a GPG signature if both content and signature are present in a database row.
* **Params:**
  * `row` — database row containing `Content` and `Signature` columns.
* **Returns:** The `Content` bytes (for writing to disk regardless of verify outcome).
* **Side Effects:**
  * Creates temporary files for content and signature.
  * Runs `gpg --batch --verify` on the signature.
  * Deletes temporary files if `isCleanupEnabled` returns true.
* **Raises:** implicitly if GPG verification fails.
* **Notes:**
  * Does nothing if either `Content` or `Signature` is empty.
  * Temporary signature file is named `<contentFileName>.asc`.
* **Example:**

  ```tcl
  set content [::Chelsea::maybeVerifySignatureWithGpg $row]
  writeFileBytes $targetFile $content
  ```

---

## Typical Workflow

**Sign files during database build:**

```tcl
::Chelsea::maybeSetupForTestGpgSigning $root

# Sign a file manually
::Chelsea::signFileWithGpg /path/to/important.conf $keyId $passphraseFileName

::Chelsea::maybeCleanupForTestGpgSigning
```

**Verify during deployment:**

```tcl
# During deployFiles, each row is verified
set content [::Chelsea::maybeVerifySignatureWithGpg $row]
::Chelsea::writeFileBytes $fileName $content
```

---

## Dependencies & Environment

* **Other Chelsea helpers:** `buildExecCommand`, `evalExecCommand`, `getColumnValueEx`, `writeFileBytes`, `isCleanupEnabled`.
* **External tools:** `gpg` (GNU Privacy Guard 2.1+).
* **Files:** Default passphrase at `$path/data/keys/passphrase.txt`.

---

*Package:* `Chelsea.Gpg 1.0` · *Namespace:* `::Chelsea`
