#!/usr/bin/env bash
set -euo pipefail

ping6 fd00:fe11:deed:0::ffff -c 1

# Any UUID will do.
NODE_ID=$(uuidgen)

# Randomly created WG keys will do nicely for these
wg genkey | tee /tmp/key > /dev/null

NODE_WG_PRIVATE_KEY=$(cat /tmp/key)
NODE_WG_PUBLIC_KEY=$(cat /tmp/key | wg pubkey)

# Cleanup
rm /tmp/key

# This isn't actually the public IP. This is the IPv4, in-VPC, private
# IP address. For our nodes this will probably start with 172.31.*
NODE_PUB_IP=$1

# This has to be manually set!
# You will get it by connecting to the production DB, and running:
# select node_id, ip, wg_ipv6 from nodes order by wg_ipv;
# And using the next available address.
# Something like: fd00:fe11:deed::7
NODE_IPV6=$2

# This is the admin port that Orchestrator is listening on.
ORCH_PORT=8090

if [ -z "$NODE_ID" ]; then
  echo "Variable 'NODE_ID' is empty or not set"
  exit 1
fi

if [ -z "$ORCH_PORT" ]; then
  echo "Variable 'ORCH_PORT' is empty or not set"
  exit 1
fi

if [ -z "$NODE_IPV6" ]; then
  echo "Variable 'NODE_IPV6' is empty or not set"
  exit 1
fi

if [ -z "$NODE_WG_PUBLIC_KEY" ]; then
  echo "Variable 'NODE_WG_PUBLIC_KEY' is empty or not set"
  exit 1
fi

if [ -z "$NODE_WG_PRIVATE_KEY" ]; then
  echo "Variable 'NODE_WG_PRIVATE_KEY' is empty or not set"
  exit 1
fi

if [ -z "$NODE_PUB_IP" ]; then
  echo "Variable 'NODE_PUB_IP' is empty or not set"
  exit 1
fi

curl "http://[fd00:fe11:deed:0::ffff]:${ORCH_PORT}/api/v1/nodes/add" \
  -v \
  -X POST \
  -H "Authorization: Bearer 3114e635-285c-4c83-be5c-9a68542f6d25" \
  -H "Content-Type: application/json" \
  -d "{
    \"node_ipv6\": \"$NODE_IPV6\",
    \"node_id\": \"$NODE_ID\",
    \"node_wg_private_key\": \"$NODE_WG_PRIVATE_KEY\",
    \"node_wg_public_key\": \"$NODE_WG_PUBLIC_KEY\",
    \"node_pub_ip\": \"$NODE_PUB_IP\"
  }" | jq .
