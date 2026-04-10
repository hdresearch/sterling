#!/bin/sh

# Build everything, push the results to S3
set -e

if [ -z "$1" ]; then
    echo "You must specify staging or production"
    exit 1
fi

if [ "$1" != "staging" ] && [ "$1" != "production" ]; then
    echo "You must specify staging or production"
    exit 1
fi

# Ensure we are at the crate root
cd "$(dirname "$0")/../" || exit

# Get an up to date copy of the configuration repo
# If the script ever fails, the cleanup won't happen. So clean up as a preventative
rm -rf configuration
git clone --depth 1 -b "$1" git@github.com:hdresearch/configuration.git

# Ensure we have cargo
export PATH="$HOME/.cargo/bin:$PATH"

# Build
cargo build --release --bin proxy --bin chelsea --bin orchestrator --bin chelsea-agent

# Make our end result
mkdir -p ./result/bin

# Add stuff in
cp configuration/config/* ./result/bin
cp ./target/release/proxy \
   ./target/release/orchestrator \
   ./target/release/chelsea \
   ./target/release/chelsea-agent \
   ./result/bin

# Tar it up
tar --dereference --create --zstd --file release.tar.zst ./result

# Ship to S3
aws s3 cp release.tar.zst "s3://sh.vers.releases/$1-release.tar.zst"

# Cleanup
rm release.tar.zst

# Cleanup configuration repo
rm -rf configuration
