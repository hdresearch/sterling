#!/bin/bash

resp=$(ceph --keyring /etc/ceph/ceph.client.chelsea.keyring -n client.chelsea -s)

if [ ! $? -eq 0 ]; then
    echo "⚠ Ceph failure!"
    exit 1
fi

echo "$resp" | grep "HEALTH_OK" > /dev/null

if [ ! $? -eq 0 ]; then
    echo "⚠ Ceph failure!"
    exit 1
fi

echo "✓ Ceph looks ok"
