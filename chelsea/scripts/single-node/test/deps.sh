#!/bin/bash

# This is a randomish sampling only
commands=("tmux" "curl" "zstd" "firecracker" "rbd" "aws" "unzip")

for i in "${commands[@]}"; do
    if ! command -v "$i" >/dev/null 2>&1
    then
        echo "Missing Deps!"
        exit 1
    fi
done

echo "✓ Dependencies look ok"
