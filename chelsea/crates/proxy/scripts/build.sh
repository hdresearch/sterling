#!/bin/sh

set -eou pipefail

cd "$(dirname "$0")/.."

docker build -t vers-proxy .
