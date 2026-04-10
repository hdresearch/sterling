#!/bin/bash
# Run Chelsea integration tests
#
# Uses sudo for privileged operations but keeps cargo/build files user-owned.
# The test framework detects capabilities and skips tests that can't run.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

cd "$REPO_ROOT"

# Ensure Ceph is running (with timeout to avoid deadlock)
if ! timeout 5 rbd ls --id chelsea &>/dev/null 2>&1; then
    echo -e "${YELLOW}Starting Ceph VM...${NC}"
    sudo "$SCRIPT_DIR/ceph-vm.sh" start
fi

# Clean up any leftover state from previous test runs (but not Ceph VM)
echo -e "${GREEN}Cleaning up leftover state...${NC}"
# Kill test firecracker processes (in jailer dirs), not the Ceph VM
sudo pkill -9 jailer 2>/dev/null || true
for pid in $(pgrep firecracker); do
    # Only kill if it's running from /var/lib/chelsea (test VMs), not Ceph VM
    if readlink -f /proc/$pid/cwd 2>/dev/null | grep -q /var/lib/chelsea; then
        sudo kill -9 $pid 2>/dev/null || true
    fi
done
# Delete test network namespaces (vm_*), not system ones
for ns in $(ip netns list 2>/dev/null | grep -E '^vm_' | cut -d' ' -f1); do
    sudo ip netns delete "$ns" 2>/dev/null || true
done
sudo rm -rf /var/lib/chelsea/db 2>/dev/null || true

# Ensure test directories exist with correct ownership
sudo mkdir -p /var/lib/chelsea
sudo chown -R "$USER:$USER" /var/lib/chelsea

# Pre-setup: enable IP forwarding and create base iptables rules
# These are one-time operations that need root
echo -e "${GREEN}Setting up network prerequisites (requires sudo)...${NC}"
sudo sysctl -q -w net.ipv4.ip_forward=1
sudo iptables -P FORWARD ACCEPT 2>/dev/null || true

# Run tests with sudo, preserving PATH for nix
echo -e "${GREEN}Running tests with root privileges...${NC}"
sudo env "PATH=$PATH" $(which cargo) test -p chelsea_lib --test vm_lifecycle

# Fix any files that got created by root during test execution
echo -e "${GREEN}Fixing file permissions...${NC}"
sudo chown -R "$USER:$USER" "$REPO_ROOT/target" 2>/dev/null || true
sudo chown -R "$USER:$USER" /var/lib/chelsea 2>/dev/null || true

echo -e "${GREEN}Done!${NC}"
