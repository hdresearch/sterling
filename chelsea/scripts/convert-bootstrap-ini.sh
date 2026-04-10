#!/bin/bash

node_id=$(jq -r '.node_id' $1)
private_ip=$(jq -r '.node_ipv6' $1)
wg_public=$(jq -r '.wg.public' $1)
wg_private=$(jq -r '.wg.private' $1)

cat << EOF
chelsea_node_id=$node_id
chelsea_wg_private_ip=$private_ip
chelsea_wg_public_key=$wg_public
chelsea_wg_private_key=$wg_private
EOF
