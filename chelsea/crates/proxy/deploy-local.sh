#!/bin/bash
# Local deployment script for proxy service
# This script loads secrets from a .env file for local/manual deployment
#
# Usage:
#   1. Create a .env file in this directory with your secrets (see .env.example)
#   2. Run: ./deploy-local.sh <image-tag>
#
# The .env file should NOT be committed to git (it's in .gitignore)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENV_FILE="$SCRIPT_DIR/.env"

# Check if .env file exists
if [ ! -f "$ENV_FILE" ]; then
    echo "Error: .env file not found at $ENV_FILE"
    echo ""
    echo "Please create a .env file with the following variables:"
    echo "  PROXY_PRIVATE_KEY=..."
    echo "  ORCHESTRATOR_PUBLIC_KEY=..."
    echo "  DATABASE_URL=..."
    echo "  ORCHESTRATOR_PUBLIC_IP=..."
    echo "  ORCHESTRATOR_PRIVATE_IP=..."
    echo "  ORCHESTRATOR_PORT=..."
    echo ""
    echo "You can copy .env.example and fill in the values:"
    echo "  cp .env.example .env"
    echo ""
    exit 1
fi

# Load environment variables from .env file
echo "Loading environment variables from $ENV_FILE..."
set -a
source "$ENV_FILE"
set +a

# Get the image tag from command line argument
if [ -z "$1" ]; then
    echo "Error: Image tag is required"
    echo "Usage: $0 <image-tag>"
    echo "Example: $0 1d06912a1f"
    exit 1
fi

export TAG="$1"

echo "========================================="
echo "Local Proxy Deployment"
echo "========================================="
echo "Image Tag: $TAG"
echo "Environment: Loaded from .env"
echo "========================================="

# Run the main deploy script
exec "$SCRIPT_DIR/deploy.sh"
