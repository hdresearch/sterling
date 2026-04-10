#! /bin/bash
# This script configures an arbitrary filesystem to be usable as a VM rootfs

set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DEFAULT_AGENT_BIN="$REPO_ROOT/target/release/chelsea-agent"

# Expected env vars:
#   FS_ROOT: Root of the filesystem to configure (required)
#   CHELSEA_AGENT_BIN: Optional path to a pre-built chelsea-agent binary. Defaults
#                      to $REPO_ROOT/target/release/chelsea-agent.

resolve_agent_bin() {
    if [ -n "${CHELSEA_AGENT_BIN:-}" ]; then
        if [ -f "$CHELSEA_AGENT_BIN" ]; then
            echo "$CHELSEA_AGENT_BIN"
            return
        fi
        echo "CHELSEA_AGENT_BIN is set to '$CHELSEA_AGENT_BIN' but the file does not exist." >&2
        exit 1
    fi

    if [ -f "$DEFAULT_AGENT_BIN" ]; then
        echo "$DEFAULT_AGENT_BIN"
        return
    fi

    cat >&2 <<'ERR'
chelsea-agent binary not found.
Please run `cargo build --release -p chelsea-agent` before invoking configure-image.sh,
or set CHELSEA_AGENT_BIN to a valid binary path.
ERR
    exit 1
}

AGENT_BIN="$(resolve_agent_bin)"

# Ensure base directories exist before writing files
mkdir -p "$FS_ROOT/usr/local/bin"
mkdir -p "$FS_ROOT/etc/systemd/system"

# Install chelsea-agent and accompanying service
install -m755 "$AGENT_BIN" "$FS_ROOT/usr/local/bin/chelsea-agent"
cat > "$FS_ROOT/etc/systemd/system/chelsea-agent.service" <<'UNIT'
[Unit]
Description=Chelsea in-guest agent
DefaultDependencies=no
After=sysinit.target
Wants=sysinit.target

[Service]
Type=simple
ExecStart=/usr/local/bin/chelsea-agent
Restart=on-failure

[Install]
WantedBy=sysinit.target
UNIT
mkdir -p "$FS_ROOT/etc/systemd/system/multi-user.target.wants"
mkdir -p "$FS_ROOT/etc/systemd/system/sysinit.target.wants"
ln -sf /etc/systemd/system/chelsea-agent.service \
    "$FS_ROOT/etc/systemd/system/sysinit.target.wants/chelsea-agent.service"

# Add script + service to configure networking
cat > "$FS_ROOT/usr/local/bin/fcnet-setup.sh" <<'EOF'
#!/usr/bin/env bash

main() {
    ip addr add 192.168.1.2/30 dev eth0
    ip addr add fd00:fe11:deed:1337::2/126 dev eth0

    ip link set eth0 up

    ip route add default via 192.168.1.1 dev eth0
    ip route add default via fd00:fe11:deed:1337::1 dev eth0
}
main
EOF
chmod +x "$FS_ROOT/usr/local/bin/fcnet-setup.sh"

cat > "$FS_ROOT/etc/systemd/system/fcnet.service" <<EOF
[Service]
Type=oneshot
ExecStart=/usr/local/bin/fcnet-setup.sh
[Install]
WantedBy=sshd.service
EOF

# Post the "ready" name / value pair (JSON) to the host notification endpoint.
cat > $FS_ROOT/usr/local/bin/notify-ready.sh <<'EOF'
#!/bin/sh
# This script reads chelsea_notify_boot_url_template from /proc/cmdline, which should be a URL
# containing the template string ":vm_id" (without quotes).
# ":vm_id" will be substituted out for the VM ID (from cmdline or /etc/vm_id)
set -eu

# Extract chelsea_vm_id from /proc/cmdline and write to /etc/vm_id if not present
if [ ! -f /etc/vm_id ]; then
    chelsea_vm_id=$(cat /proc/cmdline | grep -oE 'chelsea_vm_id=[^ ]+' | cut -d= -f2- || true)
    if [ -n "${chelsea_vm_id}" ]; then
        echo "${chelsea_vm_id}" > /etc/vm_id
        echo "Wrote VM ID to /etc/vm_id from kernel cmdline"
    else
        echo "chelsea_vm_id not found in /proc/cmdline and /etc/vm_id does not exist" >&2
        exit 1
    fi
fi

# Extract chelsea_notify_boot_url_template from /proc/cmdline
chelsea_notify_boot_url_template=$(cat /proc/cmdline | grep -oE 'chelsea_notify_boot_url_template=[^ ]+' | cut -d= -f2- || true)

if [ -z "${chelsea_notify_boot_url_template}" ]; then
    echo "chelsea_notify_boot_url_template not found in /proc/cmdline" >&2
    exit 1
fi

vm_id=$(cat /etc/vm_id)
url=$(echo "$chelsea_notify_boot_url_template" | sed "s/:vm_id/$vm_id/g")

echo "Sending ready notification to URL $url"
/usr/bin/curl -X POST -H "Content-Type: application/json" -d '{"tag_name" : "ready", "tag_value" : "true"}' "${url}"
EOF
chmod +x $FS_ROOT/usr/local/bin/notify-ready.sh

cat > $FS_ROOT/etc/systemd/system/notify-ready.service <<EOF
[Unit]
Description=Machine Readiness Notification
Wants=fcnet.service
After=fcnet.service
[Service]
Type=oneshot
ExecStart=/usr/local/bin/notify-ready.sh
EOF

# Add script + service to configure SSH keys from kernel cmdline
cat > "$FS_ROOT/usr/local/bin/ssh-setup.sh" <<'EOF'
#!/bin/sh

# This script reads chelsea_ssh_pubkey from /proc/cmdline and sets up authorized_keys.
# The pubkey is expected to be base64-encoded (the middle part of an openssh public key).
set -eu

# Check if authorized_keys already exists with content (e.g., from commit restore)
if [ -f /root/.ssh/authorized_keys ] && [ -s /root/.ssh/authorized_keys ]; then
    echo "SSH authorized_keys already configured, skipping"
    exit 0
fi

# Extract chelsea_ssh_pubkey from /proc/cmdline
chelsea_ssh_pubkey=$(cat /proc/cmdline | grep -oE 'chelsea_ssh_pubkey=[^ ]+' | cut -d= -f2- || true)

if [ -z "${chelsea_ssh_pubkey}" ]; then
    echo "chelsea_ssh_pubkey not found in /proc/cmdline" >&2
    exit 1
fi

# Set up .ssh directory
mkdir -p /root/.ssh
chmod 700 /root/.ssh

# Write the public key in openssh format (assuming ed25519)
echo "ssh-ed25519 ${chelsea_ssh_pubkey}" > /root/.ssh/authorized_keys
chmod 600 /root/.ssh/authorized_keys

echo "SSH authorized_keys configured from kernel cmdline"
EOF
chmod +x "$FS_ROOT/usr/local/bin/ssh-setup.sh"

cat > "$FS_ROOT/etc/systemd/system/ssh-setup.service" <<EOF
[Unit]
Description=Configure SSH keys from kernel cmdline
Before=sshd.service
[Service]
Type=oneshot
ExecStart=/usr/local/bin/ssh-setup.sh
RemainAfterExit=yes
EOF

# Create the wants directory and enable the services
mkdir -p "$FS_ROOT/etc/systemd/system/sysinit.target.wants/"
set +e
ln -s /etc/systemd/system/fcnet.service "$FS_ROOT/etc/systemd/system/sysinit.target.wants/fcnet.service"
ln -s /etc/systemd/system/ssh-setup.service "$FS_ROOT/etc/systemd/system/sysinit.target.wants/ssh-setup.service"
ln -s /etc/systemd/system/notify-ready.service "$FS_ROOT/etc/systemd/system/sysinit.target.wants/notify-ready.service"
set -e

# Install logrotate configuration for chelsea-agent exec logs
mkdir -p $FS_ROOT/etc/logrotate.d
install -m644 "$SCRIPT_DIR/chelsea-agent.logrotate" \
    $FS_ROOT/etc/logrotate.d/chelsea-agent

# Configure hostname
echo "hdr" > "$FS_ROOT/etc/hostname"

# Add default nameservers to resolv.conf
echo "nameserver 1.1.1.1" > "$FS_ROOT/etc/resolv.conf"
echo "nameserver 8.8.8.8" >> "$FS_ROOT/etc/resolv.conf"

# Install hook to source Chelsea-managed environment variables
mkdir -p "$FS_ROOT/etc/vers"
cat > "$FS_ROOT/etc/vers/env" <<'EOF'
# Managed by Chelsea. Runtime will overwrite this file with user-provided entries.
EOF
chmod 600 "$FS_ROOT/etc/vers/env"

mkdir -p "$FS_ROOT/etc/profile.d"
cat > "$FS_ROOT/etc/profile.d/vers-env.sh" <<'EOF'
#!/bin/sh
# shellcheck disable=SC1091
if [ -f /etc/vers/env ]; then
    . /etc/vers/env
fi
EOF
chmod 644 "$FS_ROOT/etc/profile.d/vers-env.sh"

# Install Chrony to keep guest clock up to date
sudo apt install chrony
cp /etc/systemd/system/chronyd.service "$FS_ROOT/etc/systemd/system/chronyd.service"
mkdir -p "$FS_ROOT/usr/lib/systemd/scripts/"
cp /usr/lib/systemd/scripts/chronyd-starter.sh "$FS_ROOT/usr/lib/systemd/scripts/chronyd-starter.sh"
cp /etc/default/chrony "$FS_ROOT/etc/default/chrony"
cp /usr/bin/chronyc "$FS_ROOT/usr/bin/"
cp /usr/sbin/chronyd "$FS_ROOT/usr/sbin/"
mkdir -p "$FS_ROOT/etc/chrony/"
touch "$FS_ROOT/etc/chrony/chrony.keys"
cp /etc/chrony/chrony.conf "$FS_ROOT/etc/chrony/"
echo 'refclock PHC /dev/ptp0 poll 3 dpoll -2 offset 0' >> "$FS_ROOT/etc/chrony/chrony.conf"
chroot $FS_ROOT /bin/sh -x <<EOF
adduser --allow-bad-names --system --disabled-password _chrony
systemctl enable chronyd
EOF

