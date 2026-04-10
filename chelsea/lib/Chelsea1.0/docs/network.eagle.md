# Chelsea Network Helpers

Procedures in the `::Chelsea` namespace for network-related utilities, including random IP address generation for testing purposes.

> **Conventions**
>
> * **Signature** shows the proc and defaults.
> * **Returns** shows the value.
> * **Notes** adds important details.

---

## Random IP Generation

### `randomIPv4`

* **Purpose:** Generate a random public IPv4 address suitable for testing.
* **Returns:** IPv4 address string in dotted-quad format (e.g., `203.45.67.89`).
* **Notes:** Excludes the following reserved/special ranges:
  * `0.0.0.0/8` — This host on this network
  * `10.0.0.0/8` — RFC 1918 private
  * `100.64.0.0/10` — Shared address space (RFC 6598)
  * `127.0.0.0/8` — Loopback
  * `169.254.0.0/16` — Link-local
  * `172.16.0.0/12` — RFC 1918 private
  * `192.0.0.0/24` — IANA special-purpose
  * `192.0.2.0/24` — TEST-NET-1 (documentation)
  * `192.88.99.0/24` — 6to4 anycast
  * `192.168.0.0/16` — RFC 1918 private
  * `198.18.0.0/15` — Benchmarking
  * `198.51.100.0/24` — TEST-NET-2 (documentation)
  * `203.0.113.0/24` — TEST-NET-3 (documentation)
  * `224.0.0.0/4` — Multicast (Class D)
  * `240.0.0.0/4` — Reserved (Class E)
  * `255.255.255.255` — Broadcast
* **Example:**

  ```tcl
  set ip [::Chelsea::randomIPv4]
  puts "Generated IP: $ip"
  # Output: Generated IP: 185.23.45.67
  ```

### `randomIPv6`

* **Purpose:** Generate a random global unicast IPv6 address suitable for testing.
* **Returns:** IPv6 address string in full format (e.g., `2a05:1234:5678:9abc:def0:1234:5678:9abc`).
* **Notes:**
  * Generates addresses in the `2000::/3` global unicast range.
  * First group is constrained to `0x2000` - `0x3fff`.
  * Excludes the following special-purpose ranges:
    * `2001::/23` — IETF protocol assignments (Teredo, benchmarking, ORCHID, docs, etc.)
    * `2002::/16` — 6to4 transition
    * `3fff::/20` — Documentation prefix
    * `2620:4f:8000::/48` — AS112 service
  * Returns 8 groups of 4 hex digits each.
* **Example:**

  ```tcl
  set ip [::Chelsea::randomIPv6]
  puts "Generated IPv6: $ip"
  # Output: Generated IPv6: 2a07:4c12:8f34:ab12:cd34:ef56:7890:abcd
  ```

---

## Typical Usage

**Generate test IP addresses:**

```tcl
# For simulating external connections
set publicIp [::Chelsea::randomIPv4]
set publicIp6 [::Chelsea::randomIPv6]

# Use in API payloads
set json [subst {{"ip_address": "$publicIp"}}]
```

**Generate multiple unique IPs:**

```tcl
set ips [list]
for {set i 0} {$i < 10} {incr i} {
  lappend ips [::Chelsea::randomIPv4]
}
```

---

## Dependencies

* **None** — Uses only built-in Tcl/Eagle commands (`expr`, `rand`, `format`, `appendArgs`).

---

*Package:* `Chelsea.Network 1.0` · *Namespace:* `::Chelsea`
