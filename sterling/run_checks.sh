#!/bin/bash
set -e

echo "Building Sterling..."
zig build

echo "Running tests..."
zig build test

echo "Testing CLI commands..."
./zig-out/bin/sterling version
./zig-out/bin/sterling generate --spec examples/petstore.yaml --config sterling.toml

echo "All checks passed!"
