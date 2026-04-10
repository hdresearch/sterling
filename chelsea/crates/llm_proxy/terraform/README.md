# llm_proxy — Terraform Deployment

Deploys the LLM proxy to **AWS ECS Fargate** behind an **internal ALB** at `tokens.internal.vers.sh`.

## Architecture

```
  VPC (internal only)
  ┌────────────────────────────────────────────────┐
  │                                                │
  │  tokens.internal.vers.sh                       │
  │        │                                       │
  │        ▼                                       │
  │  ┌──────────┐     ┌─────────┐  ┌─────────┐   │
  │  │ Internal  │────▶│ Fargate │  │ Fargate │   │
  │  │   ALB     │────▶│ Task 1  │  │ Task 2  │   │
  │  └──────────┘     └────┬────┘  └────┬────┘   │
  │                        │            │          │
  │                   ┌────┴────┐  ┌────┴──────┐  │
  │                   │ Main DB │  │  Logs DB  │  │
  │                   │ (vers   │  │ (t4g.med) │  │
  │                   │  RDS)   │  │ dedicated │  │
  │                   └─────────┘  └───────────┘  │
  └────────────────────────────────────────────────┘
                           │
                           ▼ (outbound via NAT)
                    OpenAI / Anthropic APIs
```

## Prerequisites

- AWS CLI configured with appropriate credentials
- Terraform ≥ 1.5
- Docker (for building images)
- A VPC with private subnets (NAT gateway) and at least 2 AZs

## Quick Start

```bash
# 1. Init Terraform
cd crates/llm_proxy/terraform
terraform init

# 2. Copy and fill in variables
cp production.tfvars.example production.tfvars
# Edit production.tfvars with your VPC, subnet, and SG IDs

# 3. Plan and apply
terraform plan -var-file=production.tfvars
terraform apply -var-file=production.tfvars

# 4. Set secrets (one-time)
#    Note: log-database-url is auto-populated by Terraform from the dedicated RDS instance.
aws secretsmanager put-secret-value --secret-id llm-proxy-production/openai-api-key    --secret-string "sk-..."
aws secretsmanager put-secret-value --secret-id llm-proxy-production/anthropic-api-key  --secret-string "sk-ant-..."
aws secretsmanager put-secret-value --secret-id llm-proxy-production/database-url       --secret-string "postgres://user:pass@vers.cwxoqiosmfyv.us-east-1.rds.amazonaws.com:5432/vers"
aws secretsmanager put-secret-value --secret-id llm-proxy-production/admin-api-key      --secret-string "your-admin-key"

# 5. Set the GitHub Actions secret for CI/CD
#    (the role ARN is created by Terraform in github_oidc.tf)
gh secret set AWS_DEPLOY_ROLE_ARN --body "$(terraform output -raw github_deploy_role_arn)"

# 6. Build and deploy (manual, or let CI handle it on push to production)
./deploy.sh v0.1.0
```

## Config Layering

The proxy loads config in two layers:

1. **TOML file** (`llm_proxy.production.toml`) — baked into the Docker image. Contains model routing, provider definitions, and safe defaults.
2. **Environment variables** — injected by ECS from Secrets Manager. Override sensitive values:
   - `LLM_PROXY_DATABASE_URL` → `database.url`
   - `LLM_PROXY_LOG_DATABASE_URL` → `database.log_url`
   - `LLM_PROXY_ADMIN_API_KEY` → `server.admin_api_key`
   - `OPENAI_API_KEY` — read by providers via `api_key_env`
   - `ANTHROPIC_API_KEY` — read by providers via `api_key_env`

## Updating

```bash
# Deploy a new version
./deploy.sh sha-$(git rev-parse --short HEAD)

# Or force redeploy current image
aws ecs update-service --cluster llm-proxy-production --service llm-proxy-production --force-new-deployment
```

## Observability

```bash
# Tail logs
aws logs tail /ecs/llm-proxy-production --follow

# Check service health
aws ecs describe-services --cluster llm-proxy-production --services llm-proxy-production \
  --query 'services[0].{desired:desiredCount,running:runningCount,status:status}'
```

## Files

| File | Purpose |
|------|---------|
| `main.tf` | Provider, backend, locals |
| `variables.tf` | Input variables |
| `ecr.tf` | ECR repository + lifecycle |
| `ecs.tf` | Cluster, task definition, service, autoscaling |
| `alb.tf` | Internal ALB, target group, listener |
| `iam.tf` | Execution role + task role |
| `secrets.tf` | Secrets Manager entries |
| `security_groups.tf` | ALB + ECS task SGs, RDS ingress |
| `rds_logs.tf` | Dedicated RDS Postgres for log data, password generation, secret wiring |
| `github_oidc.tf` | GitHub Actions OIDC provider, deploy role (ECR push + ECS deploy) |
| `dns.tf` | Route53 private zone + `tokens.internal.vers.sh` record |
| `outputs.tf` | ECR URL, ALB DNS, internal URL, logs DB endpoint, deploy role ARN |
| `deploy.sh` | Build → push → deploy helper |
