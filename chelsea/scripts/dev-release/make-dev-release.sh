#!/bin/bash

# Build everything using cargo, push the results to S3
set -ex

# Ensure we are at the repository root.
cd "$(dirname "$0")/../../"

# Bucket prefix
S3_URL="s3://sh.vers.releases/"

# Names for tar archives to be uploaded
PROXY_TAR="development-proxy.tar.zst"
ORCHESTRATOR_TAR="development-orchestrator.tar.zst"
CHELSEA_TAR="development-chelsea.tar.zst"

TAR_OPTIONS=(--zstd --transform 's|.*/||')

# Files to be included in each tar
PROXY_FILES="./target/release/proxy"
ORCHESTRATOR_FILES=(
    "./target/release/orchestrator"
    "./scripts/dev-release/create-api-key.sh"
)
FIRECRACKER_OUT_DIR="./externals/firecracker/build/cargo_target/$(uname -m)-unknown-linux-musl/release"
CHELSEA_FILES=(
    "./target/release/chelsea"
    "$FIRECRACKER_OUT_DIR/firecracker"
    "$FIRECRACKER_OUT_DIR/jailer"
)

# Make sure we are at the repository root.
cd "$(dirname "$0")/../../"

# Build binaries
cargo build --release
make clone-and-build-firecracker

# Proxy
tar "${TAR_OPTIONS[@]}" -cf "$PROXY_TAR" "$PROXY_FILES"
aws s3 cp "$PROXY_TAR" "$S3_URL"
rm "$PROXY_TAR"

# Orchestrator
tar "${TAR_OPTIONS[@]}" -cf "$ORCHESTRATOR_TAR" "${ORCHESTRATOR_FILES[@]}"
aws s3 cp "$ORCHESTRATOR_TAR" "$S3_URL"
rm "$ORCHESTRATOR_TAR"

# Chelsea
tar "${TAR_OPTIONS[@]}" -cf "$CHELSEA_TAR" "${CHELSEA_FILES[@]}"
aws s3 cp "$CHELSEA_TAR" "$S3_URL"
rm "$CHELSEA_TAR"
