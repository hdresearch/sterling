#!/bin/bash

host=$CHELSEA_PRIVATE_IP
port=$CHELSEA_SERVER_PORT

curl -sS "http://[${host}]:${port}/api/system/health" > /dev/null

if [ ! $? -eq 0 ]; then
    echo "⚠ Chelsea failed! (test 1)"
    exit 1
fi

curl -sS "http://[${host}]:${port}/api/system/telemetry" > /dev/null

if [ ! $? -eq 0 ]; then
    echo "⚠ Chelsea failed! (test 2)"
    exit 1
fi

uuid=$(uuid)

curl \
    -sS \
    --fail \
    -H "Content-Type: application/json" \
    --data '{"vm_config":{}, "vm_id": "'"${uuid}"'", "wireguard": { "wg_port": 36191, "private_key": "uNxF+OHrgyiJ1z5wdX5GJGXNUr3o4ojrX8T1dRIdE3g=", "public_key": "ADeMVfFzbF8Fr+Y9nPOw4D5c9SztjBWV+NMiYMahqlA=", "ipv6_address": "7b6b:6d29:2606:7aa5:29a1:4cb1:2602:025f", "proxy_public_key": "HVUSHz/z2jnrKb2stupo3E5b9rntHSwGlLES4IujngE=", "proxy_ipv6_address": "411e:0796:7ad5:76b1:39fe:353f:1f28:2f3e", "proxy_public_ip": "64.103.200.102" }}' \
    "http://[${host}]:${port}/api/vm/new" > /dev/null

if [ ! $? -eq 0 ]; then
    echo "⚠ Chelsea failed! (test 3)"
    exit 1
fi

echo "✓ Chelsea looks ok!"
