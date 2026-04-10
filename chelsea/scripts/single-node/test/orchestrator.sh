#!/bin/bash

token=$API_TOKEN
host=$ORCHESTRATOR_PRIVATE_IP
port=$ORCHESTRATOR_PORT

curl \
    --silent \
    --fail \
    -H "Authorization: Bearer ${token}" \
    -H "Host: api.vers.sh" \
    -H "Content-Type: application/json" \
    -X POST \
    --data '{"vm_config": {}}' \
    http://[${host}]:${port}/api/v1/vm/new_root > /dev/null

if [ ! $? -eq 0 ]; then
    echo "⚠ Orchestrator failed! (test 1)"
    exit 1
fi

curl \
    --silent \
    --fail \
    -H "Authorization: Bearer "${token} \
    http://[${host}]:${port}/api/v1/vms > /dev/null

if [ ! $? -eq 0 ]; then
    echo "⚠ Orchestrator failed! (test 2)"
    exit 1
fi

echo "✓ Orchestrator looks ok"
