# ── Internal ALB Security Group ───────────────────────────────────────────────

resource "aws_security_group" "alb" {
  name        = "${local.name_env}-alb"
  description = "Internal ALB for llm_proxy"
  vpc_id      = var.vpc_id

  # Accept traffic from within the VPC on HTTP
  ingress {
    description = "HTTP from VPC"
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = [data.aws_vpc.selected.cidr_block]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# ── Public ALB Security Group ─────────────────────────────────────────────────
# Only allows traffic from Cloudflare IP ranges (proxy mode).

resource "aws_security_group" "public_alb" {
  name        = "${local.name_env}-public-alb"
  description = "Public ALB for llm_proxy — Cloudflare only"
  vpc_id      = var.vpc_id

  # Cloudflare IPv4 ranges → HTTP
  dynamic "ingress" {
    for_each = toset(local.cloudflare_ipv4)
    content {
      description = "HTTP from Cloudflare"
      from_port   = 80
      to_port     = 80
      protocol    = "tcp"
      cidr_blocks = [ingress.value]
    }
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# https://www.cloudflare.com/ips-v4/#
locals {
  cloudflare_ipv4 = [
    "173.245.48.0/20",
    "103.21.244.0/22",
    "103.22.200.0/22",
    "103.31.4.0/22",
    "141.101.64.0/18",
    "108.162.192.0/18",
    "190.93.240.0/20",
    "188.114.96.0/20",
    "197.234.240.0/22",
    "198.41.128.0/17",
    "162.158.0.0/15",
    "104.16.0.0/13",
    "104.24.0.0/14",
    "172.64.0.0/13",
    "131.0.72.0/22",
  ]
}

# ── ECS Task Security Group ──────────────────────────────────────────────────

resource "aws_security_group" "ecs_tasks" {
  name        = "${local.name_env}-ecs-tasks"
  description = "ECS Fargate tasks for llm_proxy"
  vpc_id      = var.vpc_id

  # Accept traffic from internal ALB
  ingress {
    description     = "Traffic from internal ALB"
    from_port       = var.container_port
    to_port         = var.container_port
    protocol        = "tcp"
    security_groups = [aws_security_group.alb.id]
  }

  # Accept traffic from public ALB
  ingress {
    description     = "Traffic from public ALB"
    from_port       = var.container_port
    to_port         = var.container_port
    protocol        = "tcp"
    security_groups = [aws_security_group.public_alb.id]
  }

  # Outbound: needs to reach RDS, Secrets Manager, ECR, and LLM provider APIs
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# ── Allow ECS tasks → RDS ────────────────────────────────────────────────────
# If a database SG is provided, add an ingress rule so tasks can reach Postgres.

resource "aws_security_group_rule" "ecs_to_rds" {
  count = var.database_security_group_id != "" ? 1 : 0

  type                     = "ingress"
  from_port                = 5432
  to_port                  = 5432
  protocol                 = "tcp"
  security_group_id        = var.database_security_group_id
  source_security_group_id = aws_security_group.ecs_tasks.id
  description              = "llm_proxy ECS tasks to Postgres"
}

# ── Data source ───────────────────────────────────────────────────────────────

data "aws_vpc" "selected" {
  id = var.vpc_id
}
