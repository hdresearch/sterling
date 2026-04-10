#!/bin/bash
set -euo pipefail

MODULES_DIR="/lib/modules/$(uname -r)"
TMP_DIR="$(mktemp -d)"

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

start_udevd() {
    if command -v udevd >/dev/null 2>&1; then
        mkdir -p /run/udev
        udevd --daemon --resolve-names=never
    elif [ -x /lib/systemd/systemd-udevd ]; then
        mkdir -p /run/udev
        /lib/systemd/systemd-udevd --daemon --resolve-names=never
    else
        return
    fi

    if command -v udevadm >/dev/null 2>&1; then
        udevadm trigger --type=subsystems --action=add || true
        udevadm trigger --type=devices --action=add || true
        udevadm settle || true
    fi
}

start_udevd

exec "$@"
