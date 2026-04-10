# Chelsea WireGuard Helpers

Procedures in the `::Chelsea` namespace for WireGuard VPN configuration and management during testing. Provides configuration templates, key generation, and interface lifecycle management.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Params** lists arguments and expectations.
> * **Returns** shows the value.
> * **Notes** adds side-effects or dependencies.

---

## Configuration Template

### `getWireGuardConfigurationTemplateForTest`

* **Purpose:** Get a WireGuard configuration file template for testing.
* **Returns:** Template string with the following placeholders:
  * `%LocalPrivateIp%` — local interface address.
  * `%LocalPrivateKey%` — local WireGuard private key.
  * `%Peer1PublicKey%` — first peer's public key.
  * `%Peer1PublicIp%` — first peer's public IP.
  * `%Peer1PublicPort%` — first peer's WireGuard port.
  * `%Peer1PrivateIp%` — first peer's allowed IP (with /128 CIDR).
  * `%Peer2PublicKey%` — second peer's public key.
  * `%Peer2PublicIp%` — second peer's public IP.
  * `%Peer2PublicPort%` — second peer's WireGuard port.
  * `%Peer2PrivateIp%` — second peer's allowed IP (with /128 CIDR).
* **Template Structure:**

  ```ini
  [Interface]
  Address = %LocalPrivateIp%
  PrivateKey = %LocalPrivateKey%

  [Peer]
  PublicKey = %Peer1PublicKey%
  Endpoint = %Peer1PublicIp%:%Peer1PublicPort%
  AllowedIPs = %Peer1PrivateIp%/128

  [Peer]
  PublicKey = %Peer2PublicKey%
  Endpoint = %Peer2PublicIp%:%Peer2PublicPort%
  AllowedIPs = %Peer2PrivateIp%/128
  ```

---

## Variable Setup/Cleanup

### `setupWireGuardJsonTemplateVariables`

* **Purpose:** Generate WireGuard keys and addresses for testing.
* **Side Effects:** Sets the following variables in caller's scope:
  * `private_key1` — first WireGuard private key.
  * `private_key2` — second WireGuard private key.
  * `public_key1` — first WireGuard public key (derived from private_key1).
  * `public_key2` — second WireGuard public key (derived from private_key2).
  * `ipv6_address1` — random IPv6 address.
  * `ipv6_address2` — random IPv6 address.
  * `ipv4_address1` — random IPv4 address.
  * `port1` — port number (initialized to 0).
* **Notes:**
  * Uses `wg genkey` to generate private keys.
  * Uses `wg pubkey` to derive public keys from private keys.
  * Uses `randomIPv6` and `randomIPv4` for addresses.
* **Example:**

  ```tcl
  ::Chelsea::setupWireGuardJsonTemplateVariables
  puts "Private Key 1: $private_key1"
  puts "Public Key 1: $public_key1"
  puts "IPv6 Address: $ipv6_address1"
  ```

### `cleanupWireGuardJsonTemplateVariables`

* **Purpose:** Unset WireGuard template variables from caller's scope.
* **Side Effects:** Unsets:
  * `private_key1`, `private_key2`
  * `public_key1`, `public_key2`
  * `ipv6_address1`, `ipv6_address2`
  * `ipv4_address1`
  * `port1`
* **Example:**

  ```tcl
  ::Chelsea::cleanupWireGuardJsonTemplateVariables
  ```

---

## Interface Management

### `wgQuickUp channel fileName`

* **Purpose:** Bring up a WireGuard interface using a configuration file.
* **Params:**
  * `channel` — output channel for error messages.
  * `fileName` — path to WireGuard configuration file.
* **Side Effects:** Runs `wg-quick up <fileName>`.
* **Notes:**
  * Ignores "File exists" errors (interface already up).
  * Prints error to channel and raises on other failures.
* **Example:**

  ```tcl
  ::Chelsea::wgQuickUp stdout /etc/wireguard/wg0.conf
  ```

### `wgQuickDown channel fileName`

* **Purpose:** Bring down a WireGuard interface and optionally delete the config file.
* **Params:**
  * `channel` — output channel for warning messages.
  * `fileName` — path to WireGuard configuration file.
* **Side Effects:**
  * Runs `wg-quick down <fileName>`.
  * Deletes the config file if cleanup is enabled.
* **Notes:**
  * Prints warning (not error) on failure.
  * Uses `isCleanupEnabled` to determine file deletion.
* **Example:**

  ```tcl
  ::Chelsea::wgQuickDown stdout /etc/wireguard/wg0.conf
  ```

---

## Typical Workflow

**Set up WireGuard for testing:**

```tcl
# Generate keys and addresses
::Chelsea::setupWireGuardJsonTemplateVariables

# Get the template
set template [::Chelsea::getWireGuardConfigurationTemplateForTest]

# Substitute values
set config [string map [list \
    %LocalPrivateIp% "10.0.0.1/24" \
    %LocalPrivateKey% $private_key1 \
    %Peer1PublicKey% $public_key2 \
    %Peer1PublicIp% $ipv4_address1 \
    %Peer1PublicPort% 51820 \
    %Peer1PrivateIp% $ipv6_address1 \
    %Peer2PublicKey% "..." \
    %Peer2PublicIp% "..." \
    %Peer2PublicPort% 51821 \
    %Peer2PrivateIp% "..." \
] $template]

# Write config and bring up interface
set confFile "/tmp/wg-test.conf"
writeFile $confFile $config
::Chelsea::wgQuickUp stdout $confFile

# ... run tests ...

# Cleanup
::Chelsea::wgQuickDown stdout $confFile
::Chelsea::cleanupWireGuardJsonTemplateVariables
```

---

## Dependencies & Environment

* **Other Chelsea helpers:** `buildExecCommand`, `evalExecCommand`, `isCleanupEnabled`, `randomIPv4`, `randomIPv6`.
* **External tools:**
  * `wg` — WireGuard userspace tools (for `genkey`, `pubkey`).
  * `wg-quick` — WireGuard quick configuration tool.
* **Permissions:** May require root/sudo for interface management.

---

*Package:* `Chelsea.WireGuard 1.0` · *Namespace:* `::Chelsea`
