#!/bin/sh

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                                                                            │
# │   Build a static version of the agent using docker                         │
# │                                                                            │
# └────────────────────────────────────────────────────────────────────────────┘

# Make sure we are at the repository root.
cd "$(dirname "$0")/../" || exit

# Built image
docker build -t chelsea-agent -f Dockerfile.agent .

# Run it so we can copy from it
docker run --init -d --name chelsea-agent chelsea-agent sleep infinity

# Copy the build from it
docker cp chelsea-agent:/usr/src/agent/target/release/chelsea-agent .

# Stop
docker stop chelsea-agent

# Remove
docker rm chelsea-agent


