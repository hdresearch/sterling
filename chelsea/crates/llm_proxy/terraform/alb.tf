# ── Internal Application Load Balancer ────────────────────────────────────────
# Used by VPC-internal traffic (e.g. Vers VMs calling the proxy).

resource "aws_lb" "internal" {
  name               = local.name_env
  internal           = true
  load_balancer_type = "application"
  security_groups    = [aws_security_group.alb.id]
  subnets            = var.public_subnet_ids

  enable_deletion_protection = var.environment == "production"
}

resource "aws_lb_listener" "internal_http" {
  load_balancer_arn = aws_lb.internal.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.llm_proxy.arn
  }
}

# ── Public Application Load Balancer ──────────────────────────────────────────
# Internet-facing, sits behind Cloudflare proxy (which handles TLS).
# Cloudflare → ALB on HTTP (port 80). Cloudflare IPs are allowed in the SG.

resource "aws_lb" "public" {
  name               = "${local.name_env}-public"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.public_alb.id]
  subnets            = var.public_subnet_ids

  enable_deletion_protection = var.environment == "production"
}

resource "aws_lb_listener" "public_http" {
  load_balancer_arn = aws_lb.public.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.llm_proxy.arn
  }
}

# ── Shared Target Group ───────────────────────────────────────────────────────
# Both ALBs forward to the same target group / ECS tasks.

resource "aws_lb_target_group" "llm_proxy" {
  name        = local.name_env
  port        = var.container_port
  protocol    = "HTTP"
  vpc_id      = var.vpc_id
  target_type = "ip" # Required for Fargate awsvpc networking

  health_check {
    enabled             = true
    path                = "/health"
    port                = "traffic-port"
    protocol            = "HTTP"
    healthy_threshold   = 2
    unhealthy_threshold = 3
    timeout             = 5
    interval            = 15
    matcher             = "200"
  }

  deregistration_delay = 30

  lifecycle {
    create_before_destroy = true
  }
}
