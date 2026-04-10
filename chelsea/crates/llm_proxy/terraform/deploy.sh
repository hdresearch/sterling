#!/usr/bin/env bash
#
# Build, push, and deploy llm_proxy to ECS Fargate.
#
# Usage:
#   ./deploy.sh                  # Deploy with "latest" tag
#   ./deploy.sh v1.2.3           # Deploy with specific tag
#   ./deploy.sh sha-abc1234      # Deploy a git SHA tag
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
TAG="${1:-latest}"

# Get ECR URL from Terraform output
cd "$SCRIPT_DIR"
ECR_URL=$(terraform output -raw ecr_repository_url)
CLUSTER=$(terraform output -raw ecs_cluster_name)
SERVICE=$(terraform output -raw ecs_service_name)
REGION="us-east-1"

echo "📦 Building llm_proxy image (tag: $TAG)..."
cd "$REPO_ROOT"
docker build -f crates/llm_proxy/Dockerfile -t "llm-proxy:$TAG" .

echo "🔑 Logging into ECR..."
aws ecr get-login-password --region "$REGION" | docker login --username AWS --password-stdin "$ECR_URL"

echo "🚀 Pushing to ECR..."
docker tag "llm-proxy:$TAG" "$ECR_URL:$TAG"
docker push "$ECR_URL:$TAG"

echo "♻️  Forcing new ECS deployment..."
aws ecs update-service \
    --region "$REGION" \
    --cluster "$CLUSTER" \
    --service "$SERVICE" \
    --force-new-deployment \
    --no-cli-pager

echo "✅ Deploy triggered. Watch with:"
echo "   aws ecs wait services-stable --region $REGION --cluster $CLUSTER --services $SERVICE"
