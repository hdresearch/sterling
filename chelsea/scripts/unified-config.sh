#!/bin/bash

# Make sure we are at the repository root.
cd "$(dirname "$0")/../" || exit

config_path="config/"

files=("500-common.ini" "520-chelsea.ini" "540-orchestrator.ini" "560-proxy.ini")

rm /tmp/unified.ini

for file in "${files[@]}"; do
    cat "$config_path$file" >> /tmp/unified.ini
done

cat /tmp/unified.ini | grep -v '^#.*' | sort | uniq > unified.ini


