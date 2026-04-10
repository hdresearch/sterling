#!/bin/bash

set -e

# Set defaults for optional variables
export PROXY_PORT="${PROXY_PORT:-80}"
export SSH_PORT="${SSH_PORT:-80}"
export ADMIN_PORT="${ADMIN_PORT:-9090}"
export PROXY_WG_PORT="${PROXY_WG_PORT:-51822}"
export RUST_LOG="${RUST_LOG:-info}"

echo "========================================="
echo "Deploying proxy"
echo "========================================="
echo "Image Tag: $TAG"
echo "Proxy Port: $PROXY_PORT (receives traffic from ELB)"
echo "SSH Port: $SSH_PORT"
echo "Admin Port: $ADMIN_PORT"
echo "Log Level: $RUST_LOG"
echo "========================================="

# Navigate to the directory containing docker-compose.yml
cd "$(dirname "$0")"

# Pull the latest image
echo "Pulling image..."
docker compose pull

# Stop and remove any existing container
echo "Stopping existing proxy container..."
docker compose down 2>/dev/null || true

# Start the service
echo "Starting proxy service..."
docker compose up -d

# Wait a moment for startup
sleep 3

# Show status
echo ""
echo "Container status:"
docker compose ps

echo ""
echo "Recent logs:"
docker compose logs --tail=30

echo ""
echo "========================================="
echo "Deployment complete!"
echo "========================================="
echo "To view live logs: docker compose logs -f"
echo "To stop: docker compose down"
