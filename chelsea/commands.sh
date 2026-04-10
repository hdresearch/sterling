#!/bin/bash

DATA_DIR=${DATA_DIR:-/var/lib/chelsea}

DB_DIR=${DB_DIR:-$DATA_DIR/db}
DB_FILE=$DB_DIR/chelsea.db

# Cleanup function
cleanup() {
    echo "Cleaning up..."

    # Unmap RBDs FIRST - must happen before killing firecracker/cloud-hypervisor
    # because Ceph runs inside a Firecracker VM. If we kill the VM first,
    # the unmap commands will hang indefinitely waiting for a dead cluster.
    echo "Unmapping RBD devices..."
    count=0
    for dev in $(rbd showmapped 2>/dev/null | grep -oP '/dev/rbd\d+'); do
        if [[ -b "$dev" ]]; then
            sudo rbd --id chelsea device unmap "$dev" 2>/dev/null || true
            ((count++))
        fi
    done
    echo "Unmapped $count RBD device(s)"

    # VM process cleanup
    echo "Killing cloud-hypervisor processes..."
    sudo pkill -9 -f cloud-hypervisor 2>/dev/null || true
    count=$(ps aux | grep -c cloud-hypervisor | grep -v grep || echo 0)
    echo "killed cloud-hypervisor (remaining: $count)"

    echo "Killing firecracker processes..."
    sudo pkill -9 -f firecracker 2>/dev/null || true
    count=$(ps aux | grep -c firecracker | grep -v grep || echo 0)
    echo "killed firecracker (remaining: $count)"

    # WireGuard interface cleanup
    echo "Cleaning up WireGuard interfaces..."
    for wg in wgproxy wgchelsea wgorchestrator; do
        sudo ip link delete $wg 2>/dev/null || true
    done
    echo "WireGuard interfaces cleaned"

    # VM process cleanup
    echo "Killing VM processes..."
    count=0
    for process in $(sqlite3 $DB_FILE "SELECT vm_process_pid FROM vm;"); do
        sudo kill $process
        ((count++))
    done
    echo "found $count"

    echo "Killing chelsea processes..."
    count=0
    for process in $(ps -A | grep chelsea | awk '{print $1}'); do
        sudo kill $process
        ((count++))
    done
    echo "found $count"

    # Network cleanup
    echo "Cleaning up network namespaces..."
    for ns in $(sudo ip netns list | awk '{print $1}' | grep '^vm_'); do
        sudo ip netns delete "$ns"
    done

    # NAT cleanup
    echo "Cleaning up NAT table..."
    sudo iptables -t nat -S PREROUTING | grep -- '--to-destination [0-9.]\+:22' | while read -r rule; do
        # Convert -A to -D to delete the rule
        delete_rule=$(echo "$rule" | sed 's/^-A /-D /')
        sudo iptables -t nat $delete_rule
    done

    # Flush nftables chelsea_nat table to prevent duplicate rules
    echo "Flushing nftables chelsea_nat table..."
    sudo nft flush table ip chelsea_nat 2>/dev/null || true

    # Jail cleanup
    echo "Cleaning up jail roots..."
    sudo rm -r /srv/jailer/firecracker 2>/dev/null || true

    # Database cleanup
    echo "Cleaning up database..."
    sudo rm -r $DB_DIR 2>/dev/null || true

    # Data dir cleanup
    echo "Cleaning up data dir..."
    sudo rm -r $DATA_DIR/commits $DATA_DIR/monitor_logs $DATA_DIR/process_logs $DATA_DIR/vm_logs 2>/dev/null || true
}

# Helper function to set up SSH connection details; sets KEY_FILE and PEER_ADDR to be used by the ssh command
setup_ssh_connection() {
    local VM_ID=$1

    HOST_ADDR_U32=$(sqlite3 $DB_FILE "SELECT vm_network_host_addr FROM vm WHERE id = '$VM_ID'")
    if [ -z "$HOST_ADDR_U32" ]; then
        echo "No host address found for VM id $VM_ID"
        return 1
    fi

    # Convert u32 to IPv4
    IP1=$(( (HOST_ADDR_U32 >> 24) & 0xFF ))
    IP2=$(( (HOST_ADDR_U32 >> 16) & 0xFF ))
    IP3=$(( (HOST_ADDR_U32 >> 8) & 0xFF ))
    IP4=$(( (HOST_ADDR_U32 & 0xFF) + 1 ))
    PEER_ADDR="$IP1.$IP2.$IP3.$IP4"
    echo "VM $VM_ID peer address: $PEER_ADDR"

    PRIVATE_KEY=$(sqlite3 $DB_FILE "SELECT ssh_private_key FROM vm WHERE id = '$VM_ID'")
    if [ -z "$PRIVATE_KEY" ]; then
        echo "No SSH private key found for VM id $VM_ID"
        return 1
    fi

    # Create temporary file containing private key
    KEY_FILE=$(mktemp -p /dev/shm)
    trap "rm $KEY_FILE" EXIT
    echo "$PRIVATE_KEY" > $KEY_FILE
    chmod 600 $KEY_FILE
}

connect() {
    local VM_ID=$1

    setup_ssh_connection "$VM_ID" || return 1

    ssh -i $KEY_FILE -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o PasswordAuthentication=no -o IdentitiesOnly=yes root@$PEER_ADDR
}

execute() {
    local VM_ID=$1
    shift  # Remove vm_id from args, leaving just the command

    if [ $# -eq 0 ]; then
        echo "Usage: $0 execute <vm_id> <command> [args...]"
        return 1
    fi

    setup_ssh_connection "$VM_ID" || return 1

    ssh -i $KEY_FILE -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o PasswordAuthentication=no -o IdentitiesOnly=yes root@$PEER_ADDR "$@"
}

# Main command handling
case "$1" in
    cleanup)
        cleanup
        ;;
    connect)
        if [ -z "$2" ]; then
            echo "Usage: $0 connect <vm_id>"
            exit 1
        fi
        connect "$2"
        ;;
    execute)
        if [ -z "$2" ]; then
            echo "Usage: $0 execute <vm_id> <command> [args...]"
            exit 1
        fi
        VM_ID="$2"
        shift 2  # Remove script name and vm_id, leaving command args
        execute "$VM_ID" "$@"
        ;;
    *)
        echo "Usage: $0 cleanup | connect <vm_id> | execute <vm_id> <command> [args...]"
        exit 1
        ;;
esac