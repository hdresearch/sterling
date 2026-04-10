#!/bin/bash

set -eu

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                                                                            │
# │   Bootstrap or destroy a Ceph test cluster running inside a Firecracker   │
# │   VM. Used for integration testing that requires real Ceph storage.       │
# │                                                                            │
# └────────────────────────────────────────────────────────────────────────────┘

CEPH_CLUSTER_DIR="${CEPH_CLUSTER_DIR:-/srv/ceph-test-cluster}"
CEPH_ARCHIVE_URL="${CEPH_ARCHIVE_URL:-https://hdr-devops-public.s3.us-east-1.amazonaws.com/ceph-test-cluster.tar.zst}"
CEPH_ARCHIVE_FILE="${CEPH_CLUSTER_DIR}.tar.zst"
CEPH_SSH_HOST="root@172.16.0.2"
CEPH_CONF_DIR="/etc/ceph"

usage() {
    cat <<EOF

Usage: $0 <command>

Commands:
    start       Download (if needed), extract, and start the Ceph VM
    stop        Gracefully stop the Ceph VM
    destroy     Stop the VM and remove all Ceph data
    status      Check if Ceph is running and accessible
    reset-pool  Reset the rbd pool (deletes all images!)

Environment variables:
    CEPH_CLUSTER_DIR   Where to store the cluster (default: /srv/ceph-test-cluster)
    CEPH_ARCHIVE_URL   URL to download the cluster archive from

EOF
    exit 1
}

check_root() {
    if [ "$(id -u)" -ne 0 ]; then
        echo "Error: This script must be run as root"
        exit 1
    fi
}

setup_ceph_config() {
    echo "Setting up Ceph client configuration..."
    mkdir -p "$CEPH_CONF_DIR"

    cat > "$CEPH_CONF_DIR/ceph.conf" <<'EOT'
[global]
    fsid = be4d1849-9fc1-11f0-a026-0600ac100002
    mon_host = [v2:172.16.0.2:3300/0,v1:172.16.0.2:6789/0]

EOT

    cat > "$CEPH_CONF_DIR/ceph.client.chelsea.keyring" <<'EOT'
[client.chelsea]
    key = AQBt0t5o1o+ABxAA0qEJ8XZ4uQfRFZE+hiIUiA==

EOT

    echo "Ceph client configuration written to $CEPH_CONF_DIR"
}

download_cluster() {
    if [ -f "$CEPH_ARCHIVE_FILE" ]; then
        echo "Cluster archive already exists at $CEPH_ARCHIVE_FILE"
        return 0
    fi

    echo "Downloading Ceph test cluster archive..."
    echo "This is a large file (~2GB compressed, ~51GB extracted)"
    
    local parent_dir
    parent_dir=$(dirname "$CEPH_ARCHIVE_FILE")
    mkdir -p "$parent_dir"
    
    curl -L -o "$CEPH_ARCHIVE_FILE" "$CEPH_ARCHIVE_URL"
    echo "Download complete"
}

extract_cluster() {
    if [ -d "$CEPH_CLUSTER_DIR" ]; then
        echo "Cluster directory already exists at $CEPH_CLUSTER_DIR"
        return 0
    fi

    if [ ! -f "$CEPH_ARCHIVE_FILE" ]; then
        echo "Error: Archive file not found at $CEPH_ARCHIVE_FILE"
        echo "Run '$0 start' to download it first"
        exit 1
    fi

    echo "Extracting cluster archive to $CEPH_CLUSTER_DIR..."
    echo "This will take a while (expanding to ~51GB)..."
    
    local parent_dir
    parent_dir=$(dirname "$CEPH_CLUSTER_DIR")
    cd "$parent_dir"
    
    tar -xf "$CEPH_ARCHIVE_FILE"
    echo "Extraction complete"
}

start_ceph_vm() {
    if ! [ -d "$CEPH_CLUSTER_DIR" ]; then
        echo "Error: Cluster directory not found at $CEPH_CLUSTER_DIR"
        exit 1
    fi

    cd "$CEPH_CLUSTER_DIR"

    # Check if already running
    if pgrep -f "firecracker" > /dev/null 2>&1; then
        echo "Firecracker appears to already be running, checking Ceph..."
        local ssh_key
        ssh_key=$(ls "$CEPH_CLUSTER_DIR"/*.id_rsa 2>/dev/null | tail -1)
        if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
               -o ConnectTimeout=5 -i "$ssh_key" "$CEPH_SSH_HOST" "ceph health" 2>/dev/null; then
            echo "Ceph is already running and healthy!"
            return 0
        fi
        echo "Ceph not responding, will restart..."
        pkill -f firecracker || true
        sleep 2
    fi

    local ssh_key
    ssh_key=$(ls "$CEPH_CLUSTER_DIR"/*.id_rsa 2>/dev/null | tail -1)
    
    # Clean up any existing firecracker state
    echo "Cleaning up existing Firecracker state..."
    pkill -9 firecracker 2>/dev/null || true
    rm -f /tmp/firecracker.socket 2>/dev/null || true
    sleep 1

    echo "Starting Firecracker for Ceph VM..."
    nohup ./start-firecracker.sh > /tmp/ceph-firecracker.log 2>&1 &
    FIRECRACKER_PID=$!
    sleep 2

    # Verify Firecracker started
    if ! kill -0 $FIRECRACKER_PID 2>/dev/null; then
        echo "Error: Firecracker failed to start"
        cat /tmp/ceph-firecracker.log
        return 1
    fi

    if [ ! -S /tmp/firecracker.socket ]; then
        echo "Error: Firecracker socket not created"
        cat /tmp/ceph-firecracker.log
        return 1
    fi

    echo "Configuring VM via Firecracker API..."
    # Run start-vm.sh but strip out the interactive SSH at the end
    # We'll replicate the important parts here
    
    TAP_DEV="tap0"
    TAP_IP="172.16.0.1"
    MASK_SHORT="/30"
    API_SOCKET="/tmp/firecracker.socket"

    # Setup network interface
    ip link del "$TAP_DEV" 2>/dev/null || true
    ip tuntap add dev "$TAP_DEV" mode tap
    ip addr add "${TAP_IP}${MASK_SHORT}" dev "$TAP_DEV"
    ip link set dev "$TAP_DEV" up

    # Enable ip forwarding
    echo 1 > /proc/sys/net/ipv4/ip_forward
    iptables -P FORWARD ACCEPT

    # Get host interface for NAT
    HOST_IFACE=$(ip -j route list default | jq -r '.[0].dev')

    # Set up microVM internet access
    iptables -t nat -D POSTROUTING -o "$HOST_IFACE" -j MASQUERADE 2>/dev/null || true
    iptables -t nat -A POSTROUTING -o "$HOST_IFACE" -j MASQUERADE

    # Create log file
    touch ./firecracker.log

    # Configure Firecracker via API
    curl -s -X PUT --unix-socket "${API_SOCKET}" \
        --data '{"log_path": "./firecracker.log", "level": "Debug", "show_level": true, "show_log_origin": true}' \
        "http://localhost/logger"

    KERNEL="./$(ls vmlinux* | tail -1)"
    curl -s -X PUT --unix-socket "${API_SOCKET}" \
        --data "{\"kernel_image_path\": \"${KERNEL}\", \"boot_args\": \"console=ttyS0 reboot=k panic=1\"}" \
        "http://localhost/boot-source"

    ROOTFS="./$(ls *.ext4 | grep -v osd | tail -1)"
    curl -s -X PUT --unix-socket "${API_SOCKET}" \
        --data "{\"drive_id\": \"rootfs\", \"path_on_host\": \"${ROOTFS}\", \"is_root_device\": true, \"is_read_only\": false}" \
        "http://localhost/drives/rootfs"

    # OSD disks
    for i in 1 2 3; do
        curl -s -X PUT --unix-socket "${API_SOCKET}" \
            --data "{\"drive_id\": \"osd${i}\", \"path_on_host\": \"./osd${i}.ext4\", \"is_root_device\": false, \"is_read_only\": false}" \
            "http://localhost/drives/osd${i}"
    done

    # Network interface
    FC_MAC="06:00:AC:10:00:02"
    curl -s -X PUT --unix-socket "${API_SOCKET}" \
        --data "{\"iface_id\": \"net1\", \"guest_mac\": \"$FC_MAC\", \"host_dev_name\": \"$TAP_DEV\"}" \
        "http://localhost/network-interfaces/net1"

    # Machine config
    curl -s -X PUT --unix-socket "${API_SOCKET}" \
        --data '{"mem_size_mib": 6000, "vcpu_count": 4}' \
        "http://localhost/machine-config"

    sleep 0.5

    # Start the VM
    echo "Starting VM..."
    curl -s -X PUT --unix-socket "${API_SOCKET}" \
        --data '{"action_type": "InstanceStart"}' \
        "http://localhost/actions"

    echo "Waiting for VM to boot..."
    sleep 10

    # Debug: show network state
    echo "Debug: Network state after VM start:"
    echo "  tap0: $(ip addr show tap0 2>&1 | head -3 || echo 'not found')"
    echo "  Route table:"
    ip route | grep -E "172.16|tap0" || echo "  No relevant routes"
    echo "  Ping 172.16.0.2:"
    ping -c 1 -W 2 172.16.0.2 2>&1 || echo "  Ping failed"

    # Setup networking in guest
    echo "Configuring guest networking..."
    local retries=30
    while [ $retries -gt 0 ]; do
        if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
               -o ConnectTimeout=5 -i "$ssh_key" "$CEPH_SSH_HOST" "echo ok" 2>/dev/null; then
            # Setup routes
            ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
                -i "$ssh_key" "$CEPH_SSH_HOST" "ip route add default via 172.16.0.1 dev eth0 2>/dev/null || true"
            ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
                -i "$ssh_key" "$CEPH_SSH_HOST" "echo 'nameserver 8.8.8.8' > /etc/resolv.conf"
            break
        fi
        echo "Waiting for SSH... ($retries attempts remaining)"
        sleep 2
        retries=$((retries - 1))
    done

    if [ $retries -eq 0 ]; then
        echo "Error: Could not connect to VM via SSH"
        return 1
    fi

    echo "Waiting for Ceph to become ready (in-VM check)..."
    retries=60
    while [ $retries -gt 0 ]; do
        if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
               -o ConnectTimeout=5 -i "$ssh_key" "$CEPH_SSH_HOST" "ceph health" 2>/dev/null; then
            echo "Ceph is ready inside VM!"
            break
        fi
        echo "Waiting for Ceph... ($retries attempts remaining)"
        sleep 5
        retries=$((retries - 1))
    done

    if [ $retries -eq 0 ]; then
        echo "Warning: Ceph may not be fully ready yet (in-VM check failed)"
        return 1
    fi

    # Now verify from host side - this is what tests will actually use
    echo "Verifying Ceph is accessible from host..."
    retries=30
    while [ $retries -gt 0 ]; do
        if timeout 5 rbd ls --id chelsea >/dev/null 2>&1; then
            echo "Ceph is accessible from host!"
            return 0
        fi
        echo "Waiting for host-side Ceph access... ($retries attempts remaining)"
        sleep 2
        retries=$((retries - 1))
    done

    echo "Error: Ceph is running but not accessible from host"
    echo "Debug info:"
    echo "  - tap0 status: $(ip link show tap0 2>&1 || echo 'not found')"
    echo "  - Route to 172.16.0.2: $(ip route get 172.16.0.2 2>&1 || echo 'no route')"
    echo "  - Ping test: $(ping -c 1 -W 2 172.16.0.2 2>&1 || echo 'ping failed')"
    return 1
}

stop_ceph_vm() {
    echo "Stopping Ceph VM..."
    
    local ssh_key
    ssh_key=$(ls "$CEPH_CLUSTER_DIR"/*.id_rsa 2>/dev/null | tail -1)
    
    if [ -n "$ssh_key" ] && [ -f "$ssh_key" ]; then
        ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
            -o ConnectTimeout=5 -i "$ssh_key" "$CEPH_SSH_HOST" "reboot" 2>/dev/null || true
    fi

    echo "Waiting for VM to shut down..."
    sleep 3

    # Kill any remaining firecracker processes
    pkill -9 -f "firecracker" 2>/dev/null || true
    
    # Clean up network
    echo "Cleaning up network..."
    ip link del tap0 2>/dev/null || true
    iptables -t nat -D POSTROUTING -o "$(ip -j route list default | jq -r '.[0].dev' 2>/dev/null || echo eth0)" -j MASQUERADE 2>/dev/null || true
    
    # Clean up Firecracker socket
    rm -f /tmp/firecracker.socket 2>/dev/null || true
    
    echo "Ceph VM stopped"
}

destroy_ceph() {
    echo "Destroying Ceph cluster..."
    
    stop_ceph_vm
    
    if [ -d "$CEPH_CLUSTER_DIR" ]; then
        echo "Removing cluster directory: $CEPH_CLUSTER_DIR"
        rm -rf "$CEPH_CLUSTER_DIR"
    fi
    
    if [ -f "$CEPH_ARCHIVE_FILE" ]; then
        echo "Removing archive file: $CEPH_ARCHIVE_FILE"
        rm -f "$CEPH_ARCHIVE_FILE"
    fi
    
    echo "Ceph cluster destroyed"
}

check_status() {
    echo "Checking Ceph status..."
    
    if ! [ -d "$CEPH_CLUSTER_DIR" ]; then
        echo "Status: NOT INSTALLED (cluster directory not found)"
        return 1
    fi

    if ! pgrep -f "firecracker.*ceph" > /dev/null 2>&1; then
        echo "Status: STOPPED (Firecracker not running)"
        return 1
    fi

    local ssh_key
    ssh_key=$(ls "$CEPH_CLUSTER_DIR"/*.id_rsa 2>/dev/null | tail -1)
    
    if [ -z "$ssh_key" ] || ! [ -f "$ssh_key" ]; then
        echo "Status: ERROR (SSH key not found)"
        return 1
    fi

    if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
           -o ConnectTimeout=5 -i "$ssh_key" "$CEPH_SSH_HOST" "ceph health" 2>/dev/null; then
        echo "Status: RUNNING"
        return 0
    else
        echo "Status: STARTING (VM running but Ceph not ready)"
        return 1
    fi
}

reset_pool() {
    echo "Resetting Ceph rbd pool..."
    
    local ssh_key
    ssh_key=$(ls "$CEPH_CLUSTER_DIR"/*.id_rsa 2>/dev/null | tail -1)
    
    if [ -z "$ssh_key" ] || ! [ -f "$ssh_key" ]; then
        echo "Error: SSH key not found"
        exit 1
    fi

    ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -i "$ssh_key" "$CEPH_SSH_HOST" bash <<'EOF'
set -e
ceph config set mon mon_allow_pool_delete true
ceph osd pool delete rbd rbd --yes-i-really-really-mean-it
ceph osd pool create rbd
rbd pool init
ceph config set mon mon_allow_pool_delete false
echo "Pool reset complete"
EOF
}

# Main
[ $# -lt 1 ] && usage

COMMAND=$1

case $COMMAND in
    start)
        check_root
        setup_ceph_config
        download_cluster
        extract_cluster
        start_ceph_vm
        echo ""
        echo "Ceph VM started successfully!"
        echo "You can now use Ceph with client.chelsea credentials"
        ;;
    stop)
        check_root
        stop_ceph_vm
        ;;
    destroy)
        check_root
        destroy_ceph
        ;;
    status)
        check_status
        ;;
    reset-pool)
        check_root
        reset_pool
        ;;
    *)
        usage
        ;;
esac
