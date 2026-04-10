#!/bin/bash

set -e

# When testing without wireguard (invalid/missing /var/lib/chelsea/bootstrap/config.json) use IPv4
# BASE_URL="http://0.0.0.0:8111/api"
BASE_URL="http://[fd00:fe11:deed::100]:8111/api"


function usage() {
    echo "Available commands: health, telemetry, new, commit, run-commit, delete, list, status, pause, resume, ssh-key, version, sleep, wake, resize-disk"
    echo "WireGuard commands: admin-wireguard-get, admin-wireguard-add-peer [json], admin-wireguard-del-peer <public_key>, admin-vm-wireguard-get <vm_id>, admin-vm-wireguard-add-peer <vm_id> [json], admin-vm-wireguard-del-peer <vm_id> <public_key>, admin-vm-network-get <vm_id>"
    echo "Image commands: image-list, image-create <name> <docker_ref> [--size SIZE_MIB], image-status <name>, image-delete <name>"
    echo "Available utilities: random-wireguard, random-wireguard-peer"
    exit 1
}

randomIpv4() {
    oct1=$((RANDOM % 256))
    oct2=$((RANDOM % 256))
    oct3=$((RANDOM % 256))
    oct4=$((RANDOM % 256))
    echo "$oct1.$oct2.$oct3.$oct4"
}

randomIpv6() {
    local segments=()
    for i in {1..8}; do
        segment=$(printf "%04x" $((RANDOM % 65536)))
        segments+=("$segment")
    done
    (IFS=:; echo "${segments[*]}")
}

randomWireguardConfig() {
    # Generate temporary keypairs
    privateKey1=$(wg genkey)
    publicKey1=$(echo "$privateKey1" | wg pubkey)
    privateKey2=$(wg genkey)
    publicKey2=$(echo "$privateKey2" | wg pubkey)

    # Generate random IP addresses
    randIpv6_1=$(randomIpv6)
    randIpv6_2=$(randomIpv6)
    randIpv4=$(randomIpv4)

    # Output as JSON
    cat <<EOF
{
    "private_key": "$privateKey1",
    "public_key": "$publicKey1",
    "ipv6_address": "$randIpv6_1",
    "proxy_public_key": "$publicKey2",
    "proxy_ipv6_address": "$randIpv6_2",
    "proxy_public_ip": "$randIpv4",
    "wg_port": 0
}
EOF
}

randomWireguardPeerPayload() {
    local privateKey
    privateKey=$(wg genkey)
    local publicKey
    publicKey=$(echo "$privateKey" | wg pubkey)
    local allowedIpv6
    allowedIpv6=$(randomIpv6)
    local allowedIpv4
    allowedIpv4=$(randomIpv4)
    local endpointIp
    endpointIp=$(randomIpv4)

    cat <<EOF
{
    "public_key": "$publicKey",
    "preshared_key": null,
    "endpoint": "$endpointIp:51820",
    "allowed_ips": [
        "$allowedIpv6/128",
        "$allowedIpv4/32"
    ],
    "persistent_keepalive_interval": 25
}
EOF
}

randomCommitPayload() {
    local commitId
    commitId=$(uuidgen)

    cat <<EOF
{
    "commit_id": "$commitId"
}
EOF
}

case "$1" in
    health)
        curl -sS "$BASE_URL/system/health"
        ;;
    telemetry)
        curl -sS "$BASE_URL/system/telemetry"
        ;;
    
    new)
        # Usage:
        #   ./api.sh new [--kernel KERNEL] [--image IMAGE] [--vcpu N] [--mem MEM_MIB] [--disk DISK_MIB] [--wait-boot]
        # Example:
        #   ./api.sh new --kernel default.bin --image default --vcpu 2 --mem 768 --disk 2048 --wait-boot

        VM_ID=$(uuidgen)

        # Default values
        KERNEL_NAME=""
        IMAGE_NAME=""
        VCPU_COUNT=""
        MEM_SIZE_MIB=""
        FS_SIZE_MIB=""
        WAIT_BOOT=false

        # Parse flags
        while [[ $# -gt 1 ]]; do
            case "$2" in
                --kernel)
                    KERNEL_NAME="$3"
                    shift 2
                    ;;
                --image)
                    IMAGE_NAME="$3"
                    shift 2
                    ;;
                --vcpu)
                    VCPU_COUNT="$3"
                    shift 2
                    ;;
                --mem)
                    MEM_SIZE_MIB="$3"
                    shift 2
                    ;;
                --disk)
                    FS_SIZE_MIB="$3"
                    shift 2
                    ;;
                --wait-boot)
                    WAIT_BOOT=true
                    shift
                    ;;
                *)
                    echo "Unknown option: $2"
                    exit 1
                    ;;
            esac
        done

        # Build the JSON payload
        JSON="{\"vm_id\":\"${VM_ID}\",\"vm_config\":{"
        SEP=""

        if [[ -n "$KERNEL_NAME" ]]; then
            JSON="$JSON$SEP\"kernel_name\":\"$KERNEL_NAME\""
            SEP=","
        fi
        if [[ -n "$IMAGE_NAME" ]]; then
            JSON="$JSON$SEP\"image_name\":\"$IMAGE_NAME\""
            SEP=","
        fi
        if [[ -n "$VCPU_COUNT" ]]; then
            JSON="$JSON$SEP\"vcpu_count\":$VCPU_COUNT"
            SEP=","
        fi
        if [[ -n "$MEM_SIZE_MIB" ]]; then
            JSON="$JSON$SEP\"mem_size_mib\":$MEM_SIZE_MIB"
            SEP=","
        fi
        if [[ -n "$FS_SIZE_MIB" ]]; then
            JSON="$JSON$SEP\"fs_size_mib\":$FS_SIZE_MIB"
            SEP=","
        fi

        JSON="$JSON}, \"wireguard\": $(randomWireguardConfig)}"

        curl -sS -X POST "$BASE_URL/vm/new?wait_boot=$WAIT_BOOT" \
            -H "Content-Type: application/json" \
            -d "$JSON"
        ;;

    commit)
        if [ -z "$2" ]; then
            echo "Usage: $0 commit <vm_id> [--keep-paused] [--skip-wait-boot]"
            exit 1
        fi
        VM_ID="$2"
        shift 2

        QUERY_PARAMS=""
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --keep-paused)
                    QUERY_PARAMS="${QUERY_PARAMS}${QUERY_PARAMS:+&}keep_paused=true"
                    shift
                    ;;
                --skip-wait-boot)
                    QUERY_PARAMS="${QUERY_PARAMS}${QUERY_PARAMS:+&}skip_wait_boot=true"
                    shift
                    ;;
                *)
                    echo "Unknown option: $1"
                    exit 1
                    ;;
            esac
        done

        curl -sS -X POST "$BASE_URL/vm/$VM_ID/commit${QUERY_PARAMS:+?$QUERY_PARAMS}" \
            -H "Content-Type: application/json" \
            -d "$(randomCommitPayload)"
        ;;

    run-commit)
        if [ -z "$2" ]; then
            echo "Usage: $0 run-commit <commit_id>"
            exit 1
        fi
        COMMIT_ID="$2"
        shift 2

        if [[ $# -gt 0 ]]; then
            echo "Unknown option: $1"
            exit 1
        fi

        VM_ID=$(uuidgen)
        curl -sS -X POST "$BASE_URL/vm/from_commit" \
            -H "Content-Type: application/json" \
            -d "{\"vm_id\":\"${VM_ID}\",\"commit_id\":\"$COMMIT_ID\",\"wireguard\":$(randomWireguardConfig)}"
        ;;

    pause)
        if [ -z "$2" ]; then
            echo "Usage $0 pause <vm_id> [--skip-wait-boot]"
            exit 1
        fi
        VM_ID="$2"
        shift 2

        QUERY_PARAMS=""
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --skip-wait-boot)
                    QUERY_PARAMS="?skip_wait_boot=true"
                    shift
                    ;;
                *)
                    echo "Unknown option: $1"
                    exit 1
                    ;;
            esac
        done

        if curl -sS -X PATCH "$BASE_URL/vm/$VM_ID/state$QUERY_PARAMS" \
            -H "Content-Type: application/json" \
            -d "{\"state\": \"Paused\"}"; then
            echo "VM $VM_ID successfully paused"
        else
            echo "Failed to pause VM $VM_ID"
        fi
        ;;

    resume)
        if [ -z "$2" ]; then
            echo "Usage $0 resume <vm_id> [--skip-wait-boot]"
            exit 1
        fi
        VM_ID="$2"
        shift 2

        QUERY_PARAMS=""
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --skip-wait-boot)
                    QUERY_PARAMS="?skip_wait_boot=true"
                    shift
                    ;;
                *)
                    echo "Unknown option: $1"
                    exit 1
                    ;;
            esac
        done

        if curl -sS -X PATCH "$BASE_URL/vm/$VM_ID/state$QUERY_PARAMS" \
            -H "Content-Type: application/json" \
            -d "{\"state\": \"Running\"}"; then
            echo "VM $VM_ID successfully resumed"
        else
            echo "Failed to resume VM $VM_ID"
        fi
        ;;

    list)
        curl -sS "$BASE_URL/vm"
        ;;

    status)
        if [ -z "$2" ]; then
            echo "Usage: $0 status <vm_id>"
            exit 1
        fi
        curl -sS "$BASE_URL/vm/$2"
        ;;

    delete)
        if [ -z "$2" ]; then
            echo "Usage: $0 delete <vm_id> [--skip-wait-boot]"
            exit 1
        fi
        VM_ID="$2"
        shift 2

        SKIP_WAIT_BOOT=false
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --skip-wait-boot)
                    SKIP_WAIT_BOOT=true
                    shift
                    ;;
                *)
                    echo "Unknown option: $1"
                    exit 1
                    ;;
            esac
        done

        QUERY_PARAMS=""
        if [ "$SKIP_WAIT_BOOT" = true ]; then
            QUERY_PARAMS="?skip_wait_boot=true"
        fi

        curl -sS -X DELETE "$BASE_URL/vm/$VM_ID$QUERY_PARAMS"
        ;;

    ssh-key)
        if [ -z "$2" ]; then
            echo "Usage: $0 ssh-key <vm_id>"
            exit 1
        fi
        curl -sS "$BASE_URL/vm/$2/ssh_key"
        ;;

    version)
        curl -sS "$BASE_URL/system/version"
        ;;
    sleep)
        if [ -z "$2" ]; then
            echo "Usage: $0 sleep <vm_id> [--skip-wait-boot]"
            exit 1
        fi
        VM_ID="$2"
        SKIP_WAIT_BOOT=false
        shift 2
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --skip-wait-boot)
                    SKIP_WAIT_BOOT=true
                    ;;
            esac
            shift
        done
        # skip_wait_boot: default is false (so pass skip_wait_boot=false for normal usage)
        curl -sS -X POST "$BASE_URL/vm/$VM_ID/sleep?skip_wait_boot=$SKIP_WAIT_BOOT"
        ;;
    wake)
        if [ -z "$2" ]; then
            echo "Usage: $0 wake <vm_id>"
            exit 1
        fi
        VM_ID="$2"

        curl -sS -X POST "$BASE_URL/vm/$VM_ID/wake" \
            -H "Content-Type: application/json" \
            -d "{\"wireguard\": $(randomWireguardConfig)}"
        ;;
    resize-disk)
        if [ -z "$2" ] || [ -z "$3" ]; then
            echo "Usage: $0 resize-disk <vm_id> <new_size_mib> [--skip-wait-boot]"
            exit 1
        fi
        VM_ID="$2"
        NEW_SIZE_MIB="$3"
        shift 3

        QUERY_PARAMS=""
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --skip-wait-boot)
                    QUERY_PARAMS="?skip_wait_boot=true"
                    shift
                    ;;
                *)
                    echo "Unknown option: $1"
                    exit 1
                    ;;
            esac
        done

        curl -sS -X PATCH "$BASE_URL/vm/$VM_ID/disk$QUERY_PARAMS" \
            -H "Content-Type: application/json" \
            -d "{\"fs_size_mib\": $NEW_SIZE_MIB}"
        ;;
    # Just a utility function that may come in handy?
    random-wireguard)
        echo $(randomWireguardConfig)
        ;;
    random-wireguard-peer)
        echo $(randomWireguardPeerPayload)
        ;;
    admin-wireguard-get)
        curl -sS "$BASE_URL/admin/wireguard"
        ;;
    admin-wireguard-add-peer)
        shift
        if [ $# -gt 0 ]; then
            PAYLOAD="$*"
        else
            PAYLOAD=$(randomWireguardPeerPayload)
            echo "No payload provided; using random peer config" >&2
        fi
        curl -sS -X POST "$BASE_URL/admin/wireguard/peers" \
            -H "Content-Type: application/json" \
            -d "$PAYLOAD"
        ;;
    admin-wireguard-del-peer)
        if [ -z "$2" ]; then
            echo "Usage: $0 admin-wireguard-del-peer <public_key>"
            exit 1
        fi
        curl -sS -X DELETE "$BASE_URL/admin/wireguard/peers/$2"
        ;;
    admin-vm-wireguard-get)
        if [ -z "$2" ]; then
            echo "Usage: $0 admin-vm-wireguard-get <vm_id>"
            exit 1
        fi
        curl -sS "$BASE_URL/admin/vm/$2/wireguard"
        ;;
    admin-vm-wireguard-add-peer)
        if [ -z "$2" ]; then
            echo "Usage: $0 admin-vm-wireguard-add-peer <vm_id> [json_payload]"
            exit 1
        fi
        VM_ID="$2"
        shift 2
        if [ $# -gt 0 ]; then
            PAYLOAD="$*"
        else
            PAYLOAD=$(randomWireguardPeerPayload)
            echo "No payload provided; using random peer config" >&2
        fi
        curl -sS -X POST "$BASE_URL/admin/vm/$VM_ID/wireguard/peers" \
            -H "Content-Type: application/json" \
            -d "$PAYLOAD"
        ;;
    admin-vm-wireguard-del-peer)
        if [ -z "$2" ] || [ -z "$3" ]; then
            echo "Usage: $0 admin-vm-wireguard-del-peer <vm_id> <public_key>"
            exit 1
        fi
        VM_ID="$2"
        PUBLIC_KEY="$3"
        curl -sS -X DELETE "$BASE_URL/admin/vm/$VM_ID/wireguard/peers/$PUBLIC_KEY"
        ;;
    admin-vm-network-get)
        if [ -z "$2" ]; then
            echo "Usage: $0 admin-vm-network-get <vm_id>"
            exit 1
        fi
        curl -sS "$BASE_URL/admin/vm/$2/network"
        ;;

    # Base image commands
    image-list)
        curl -sS "$BASE_URL/images"
        ;;
    image-create)
        # Usage: ./api.sh image-create <image_name> <docker_image_ref> [--size SIZE_MIB]
        if [ -z "$2" ] || [ -z "$3" ]; then
            echo "Usage: $0 image-create <image_name> <docker_image_ref> [--size SIZE_MIB]"
            exit 1
        fi
        IMAGE_NAME="$2"
        DOCKER_REF="$3"
        SIZE_MIB=512
        shift 3

        while [[ $# -gt 0 ]]; do
            case "$1" in
                --size)
                    SIZE_MIB="$2"
                    shift 2
                    ;;
                *)
                    echo "Unknown option: $1"
                    exit 1
                    ;;
            esac
        done

        curl -sS -X POST "$BASE_URL/images/create" \
            -H "Content-Type: application/json" \
            -d "{\"image_name\":\"$IMAGE_NAME\",\"source\":{\"type\":\"docker\",\"image_ref\":\"$DOCKER_REF\"},\"size_mib\":$SIZE_MIB}"
        ;;
    image-status)
        if [ -z "$2" ]; then
            echo "Usage: $0 image-status <image_name>"
            exit 1
        fi
        curl -sS "$BASE_URL/images/$2/status"
        ;;
    image-delete)
        if [ -z "$2" ]; then
            echo "Usage: $0 image-delete <image_name>"
            exit 1
        fi
        curl -sS -X DELETE "$BASE_URL/images/$2"
        ;;
    *)
        usage
        ;;
esac
