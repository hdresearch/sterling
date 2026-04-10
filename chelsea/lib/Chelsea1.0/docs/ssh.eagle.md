# Chelsea SSH Helpers

Procedures in the `::Chelsea` namespace for SSH key management and remote command execution. Used for connecting to virtual machines during testing.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Raises** lists error conditions (`error ...`).
> * **Notes** adds side-effects or dependencies.

---

## SSH Key Management

### `writeSshKeyToFile baseUri apiKey id fileName`

* **Purpose:** Fetch an SSH private key from the API and write it to a file with proper permissions.
* **Params:**
  * `baseUri` — base URI of the API server.
  * `apiKey` — API key for authentication.
  * `id` — virtual machine identifier (GUID).
  * `fileName` — path where the private key will be written.
* **Returns:** Empty string on success.
* **Raises:**
  * if `id` is not a valid VM identifier.
  * if API response is not valid JSON.
  * if `ssh_private_key` field is missing from response.
  * if the key is not a valid OpenSSH private key format.
* **Flow:**
  1. Call API: `GET /api/vm/<id>/ssh_key`
  2. Extract `ssh_private_key` from JSON response.
  3. Create empty file and set permissions to `600`.
  4. Write the private key content.
* **Example:**

  ```tcl
  ::Chelsea::writeSshKeyToFile $apiHost $apiKey $vmId /tmp/vm_key
  # Key now at /tmp/vm_key with mode 600
  ```

---

## SSH Command Execution

### `execSshCommand ip port id identityFileName withRoot args`

* **Purpose:** Execute a command on a remote VM via SSH.
* **Params:**
  * `ip` — IP address of the target VM.
  * `port` — SSH port number.
  * `id` — virtual machine identifier (for validation).
  * `identityFileName` — path to the SSH private key file.
  * `withRoot` — if `true`, execute via `sudo`.
  * `args` — command and arguments to execute on the remote host.
* **Returns:** `formatExecResults` dict, or raw result if background (`&`).
* **Raises:**
  * if `ip` is not a valid IP address.
  * if `port` is not a positive integer.
  * if `id` is not a valid VM identifier.
  * if `identityFileName` is not a valid file path.
* **SSH Options Used:**
  * `-i <identityFileName>` — use specified identity file.
  * `-q` — quiet mode (suppress warnings).
  * `-o PasswordAuthentication=false` — disable password auth.
  * `-o StrictHostKeyChecking=no` — don't verify host key.
  * `-o UserKnownHostsFile=/dev/null` — don't save host key.
  * `-p <port>` — specify port number.
* **Notes:**
  * Connects as `root` user.
  * Executes from the temporary path directory.
* **Example:**

  ```tcl
  set result [::Chelsea::execSshCommand 192.168.1.100 22 $vmId /tmp/key false uname -a]
  puts "Output: [dict get $result stdOut]"

  # Run command as root on remote:
  set result [::Chelsea::execSshCommand 192.168.1.100 22 $vmId /tmp/key true systemctl status]
  ```

---

## Typical Workflow

**SSH to a VM and run a command:**

```tcl
# Fetch the SSH key
set keyFile [file tempname]
::Chelsea::writeSshKeyToFile $apiHost $apiKey $vmId $keyFile

# Execute a command
set result [::Chelsea::execSshCommand $vmIp 22 $vmId $keyFile false {
  cat /etc/os-release
}]

# Check result
if {[dict get $result exitCode] eq "Success"} {
  puts [dict get $result stdOut]
}

# Cleanup
file delete $keyFile
```

**SSH with automatic key cleanup:**

```tcl
set keyFile [file tempname]
try {
  ::Chelsea::writeSshKeyToFile $apiHost $apiKey $vmId $keyFile
  set r [::Chelsea::execSshCommand $ip 22 $vmId $keyFile false uptime]
  puts "Uptime: [dict get $r stdOut]"
} finally {
  if {[file exists $keyFile]} {
    file delete $keyFile
  }
}
```

---

## Dependencies & Environment

* **Other Chelsea helpers:** `isValidVirtualMachineId`, `execCurlCommand`, `getDictionaryValue`, `isValidJson`, `getOrSetViaJsonPaths`, `stringIsSshPrivateKey`, `buildExecCommand`, `evalInDirectory`, `formatExecResults`, `getTemporaryPath`.
* **External tools:** `ssh`, `chmod`.

---

*Package:* `Chelsea.Ssh 1.0` · *Namespace:* `::Chelsea`
