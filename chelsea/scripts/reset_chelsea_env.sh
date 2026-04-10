#!/usr/bin/env bash
# Reset the host environment for Chelsea Firecracker runs.
# Usage: sudo ./scripts/reset_chelsea_env.sh

set -euo pipefail

require_root() {
    if [[ $EUID -ne 0 ]]; then
        echo "This script must be run with sudo/root privileges." >&2
        exit 1
    fi
}

stop_containers() {
    echo "[reset] Stopping Chelsea containers (if any)..."
    local running
    running=$(docker ps -q 2>/dev/null || true)
    if [[ -n "${running}" ]]; then
        docker rm -f ${running} >/dev/null 2>&1 || true
    fi
}

kill_processes() {
    echo "[reset] Killing stray chelsea/firecracker processes..."
    local pid
    for pattern in 'firecracker(\\s|$)' '/bin/server' 'chelsea_server2'; do
        while read -r pid; do
            [[ -z "${pid}" ]] && continue
            [[ "${pid}" -eq $$ ]] && continue
            [[ "${pid}" -eq $PPID ]] && continue
            kill -TERM "${pid}" 2>/dev/null || true
        done < <(pgrep -f "${pattern}" 2>/dev/null || true)
    done
    sleep 1
    for pattern in 'firecracker(\\s|$)' '/bin/server' 'chelsea_server2'; do
        while read -r pid; do
            [[ -z "${pid}" ]] && continue
            [[ "${pid}" -eq $$ ]] && continue
            [[ "${pid}" -eq $PPID ]] && continue
            kill -KILL "${pid}" 2>/dev/null || true
        done < <(pgrep -f "${pattern}" 2>/dev/null || true)
    done
}

unmap_rbd_devices() {
    echo "[reset] Unmapping lingering RBD devices..."
    local devices
    devices=$(rbd --id chelsea device list 2>/dev/null | awk 'NR>1 {print $NF}' || true)
    if [[ -n "${devices}" ]]; then
        while read -r dev; do
            [[ -z "${dev}" ]] && continue
            echo "  - Unmapping ${dev}"
            rbd --id chelsea device unmap "${dev}" 2>/dev/null || true
        done <<< "${devices}"
    fi
}

reset_netns() {
    echo "[reset] Cleaning /var/run/netns..."
    mkdir -p /var/run/netns
    while read -r ns; do
        [[ -z "${ns}" ]] && continue
        [[ "${ns}" =~ ^(Error:|failed|cannot) ]] && continue
        ip netns delete "${ns}" 2>/dev/null || true
    done < <(ip netns list 2>/dev/null | awk '{print $1}' || true)
}

reset_netns_finalize() {
    echo "[reset] Forcefully recreating /var/run/netns..."
    if mountpoint -q /var/run/netns; then
        umount -l /var/run/netns 2>/dev/null || true
    fi
    rm -rf /var/run/netns
    mkdir -p /var/run/netns
    mount --bind /var/run/netns /var/run/netns
    mount --make-rshared /var/run/netns
}

ensure_run_shared() {
    echo "[reset] Ensuring /run is shared..."
    mount --make-rshared /run 2>/dev/null || true
    mkdir -p /run/udev
    mount --bind /run/udev /run/udev
    mount --make-rshared /run/udev
}

cleanup_jails_and_logs() {
    echo "[reset] Removing stale jail directories and logs..."
    rm -rf /srv/jailer/firecracker 2>/dev/null || true
    rm -rf /var/lib/chelsea/process_logs/* 2>/dev/null || true
}

main() {
    require_root
    stop_containers
    kill_processes
    unmap_rbd_devices
    reset_netns
    reset_netns_finalize
    ensure_run_shared
    cleanup_jails_and_logs
    echo "[reset] Done. You can now relaunch the Chelsea container."
}

main "$@"
