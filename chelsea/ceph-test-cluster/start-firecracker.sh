#!/bin/sh

set +ex

API_SOCKET="/run/chelsea/firecracker-ceph.socket"

# Remove API unix socket
sudo rm -f $API_SOCKET

# Ensure dir exits
sudo mkdir -p $(dirname $API_SOCKET)

# Run firecracker
sudo firecracker --api-sock "${API_SOCKET}" --enable-pci
