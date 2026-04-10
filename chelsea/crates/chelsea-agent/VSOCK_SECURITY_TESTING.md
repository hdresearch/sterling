# Vsock Security — Manual Testing

This documents how to manually verify the vsock security hardening:
the CID check, connection limit, and exec timeout.

## Defense Layers

Two independent layers prevent in-VM privilege escalation via vsock:

| Layer | What | Tested by |
|-------|-------|-----------|
| Layer 1 (hypervisor) | Cloud Hypervisor / Firecracker do not route vsock connections from the guest back to the guest | Manual SNE test (below) |
| Layer 2 (agent CID check) | Agent only accepts peer CID 2 (`VMADDR_CID_HOST`) | `cargo test -p chelsea-agent --test vsock_loopback` (automated) |

Layer 2 is defense-in-depth. In our testing, Layer 1 blocks all
in-VM attacks before they reach the agent. The loopback tests on the
host exercise Layer 2 independently using the `vsock_loopback` kernel
module.

## Automated Tests

```bash
# All unit + integration tests (includes CID check, parse resilience,
# connection rejection, capability gating, exec timeout)
cargo test -p chelsea-agent

# Vsock loopback tests specifically (requires: sudo modprobe vsock_loopback)
cargo test -p chelsea-agent --test vsock_loopback

# Exec timeout tests
cargo test -p chelsea -- dto_to_agent
```

## Manual SNE Test: Happy Path + Attack

Requires a running SNE (`sudo ./scripts/single-node.sh start -d`).

### 1. Create a VM and get SSH access

```bash
VM_ID=$(uuidgen)
curl -sS --fail \
  -H "Content-Type: application/json" \
  --data '{
    "vm_config":{},
    "vm_id": "'"${VM_ID}"'",
    "wireguard": {
      "wg_port": 36191,
      "private_key": "uNxF+OHrgyiJ1z5wdX5GJGXNUr3o4ojrX8T1dRIdE3g=",
      "public_key": "ADeMVfFzbF8Fr+Y9nPOw4D5c9SztjBWV+NMiYMahqlA=",
      "ipv6_address": "7b6b:6d29:2606:7aa5:29a1:4cb1:2602:025f",
      "proxy_public_key": "HVUSHz/z2jnrKb2stupo3E5b9rntHSwGlLES4IujngE=",
      "proxy_ipv6_address": "411e:0796:7ad5:76b1:39fe:353f:1f28:2f3e",
      "proxy_public_ip": "64.103.200.102"
    }
  }' \
  "http://[fd00:fe11:deed::1]:8111/api/vm/new"

# Wait for boot
sleep 10

# Set up SSH (adjust VM_ID)
DB_FILE=/var/lib/chelsea/db/chelsea.db
HOST_ADDR_U32=$(sqlite3 $DB_FILE "SELECT vm_network_host_addr FROM vm WHERE id = '$VM_ID'")
IP1=$(( (HOST_ADDR_U32 >> 24) & 0xFF ))
IP2=$(( (HOST_ADDR_U32 >> 16) & 0xFF ))
IP3=$(( (HOST_ADDR_U32 >> 8) & 0xFF ))
IP4=$(( (HOST_ADDR_U32 & 0xFF) + 1 ))
VM_ADDR="${IP1}.${IP2}.${IP3}.${IP4}"
KEY_FILE=$(mktemp)
sqlite3 $DB_FILE "SELECT ssh_private_key FROM vm WHERE id = '$VM_ID'" > $KEY_FILE
chmod 600 $KEY_FILE
alias vm_ssh="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o IdentitiesOnly=yes -o LogLevel=ERROR -i $KEY_FILE root@$VM_ADDR"
```

### 2. Deploy the agent into the VM

```bash
scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o IdentitiesOnly=yes -o LogLevel=ERROR \
    -i $KEY_FILE result/bin/chelsea-agent root@$VM_ADDR:/usr/local/bin/chelsea-agent

vm_ssh "chmod +x /usr/local/bin/chelsea-agent && \
        nohup /usr/local/bin/chelsea-agent > /var/log/chelsea-agent.log 2>&1 &"
sleep 1
vm_ssh "pgrep chelsea-agent"  # should print a PID
```

### 3. Happy path — exec from host

```bash
curl -sS -H "Content-Type: application/json" \
  --data '{"command": ["echo", "hello from vsock"]}' \
  "http://[fd00:fe11:deed::1]:8111/api/vm/${VM_ID}/exec"
```

**Expected:** `{"exit_code":0,"stdout":"hello from vsock\n","stderr":""}`

### 4. Attack — privilege escalation from inside the VM

SSH into the VM and try to connect to the agent's vsock port:

```bash
vm_ssh 'python3 -c "
import socket, json

for name, cid in [(\"HOST\", 2), (\"GUEST_3\", 3)]:
    s = socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)
    s.settimeout(3)
    try:
        s.connect((cid, 10789))
        data = s.recv(4096)
        if data and b\"Ready\" in data:
            print(f\"{name}: VULNERABLE — got Ready event!\")
        else:
            print(f\"{name}: connected but no Ready\")
    except Exception as e:
        print(f\"{name}: blocked ({e})\")
    finally:
        s.close()
"'
```

**Expected:**
```
HOST: blocked ([Errno 104] Connection reset by peer)
GUEST_3: blocked ([Errno 19] No such device)
```

> **Note:** CID_LOCAL (1) and CID_ANY (0xFFFFFFFF) hang forever inside
> the guest — `connect()` ignores `settimeout()` for unreachable vsock
> CIDs. Skip these or wrap with `timeout 4 python3 ...`.

### 5. Verify attack didn't reach the agent

```bash
vm_ssh "cat /var/log/chelsea-agent.log"
```

**Expected:** Only `Accepted vsock connection from CID 2` entries from
the happy-path test. No `Rejected` entries — the hypervisor blocked the
attack before it reached the agent.

### 6. Verify agent still works after attack

```bash
curl -sS -H "Content-Type: application/json" \
  --data '{"command": ["echo", "still alive"]}' \
  "http://[fd00:fe11:deed::1]:8111/api/vm/${VM_ID}/exec"
```

**Expected:** `{"exit_code":0,"stdout":"still alive\n","stderr":""}`

## Test: Exec Timeout (no SNE needed)

The server clamps exec timeouts:
- `None` / `0` → 300s default
- Values > 3600s → clamped to 3600s

```bash
# Via API with a short timeout:
curl -sS -H "Content-Type: application/json" \
  --data '{"command": ["sleep", "30"], "timeout_secs": 2}' \
  "http://[fd00:fe11:deed::1]:8111/api/vm/${VM_ID}/exec"
```

**Expected:** Error response after ~2 seconds mentioning "timed out".
