#!/bin/bash

token=$API_TOKEN
# todo bug in the proxy that this isn't configurable
host=127.0.0.1
port=$PROXY_PORT

# Note that the host header is required. The proxy routes requests
# based on hostname, and api.vers.sh is the only hostname that is
# forwarded to the Orchestrator.
curl \
    --silent \
    --fail \
    -H "Authorization: Bearer ""${token}" \
    -H "Host: api.vers.sh" \
    -H "Content-Type: application/json" \
    -X POST \
    --data '{"vm_config": {}}' \
    "http://${host}:${port}/health" > /dev/null

if [ ! $? -eq 0 ]; then
    echo "⚠ Proxy failed!"
    exit 1
fi

echo "✓ Proxy looks ok"
