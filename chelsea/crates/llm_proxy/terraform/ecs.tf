# ── ECS Cluster ───────────────────────────────────────────────────────────────

resource "aws_ecs_cluster" "llm_proxy" {
  name = local.name_env

  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}

# ── CloudWatch Log Group ─────────────────────────────────────────────────────

resource "aws_cloudwatch_log_group" "llm_proxy" {
  name              = "/ecs/${local.name_env}"
  retention_in_days = var.log_retention_days
}

# ── Task Definition ──────────────────────────────────────────────────────────

resource "aws_ecs_task_definition" "llm_proxy" {
  family                   = local.name_env
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = var.cpu
  memory                   = var.memory
  execution_role_arn       = aws_iam_role.ecs_execution.arn
  task_role_arn            = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name      = "llm-proxy"
      image     = "${aws_ecr_repository.llm_proxy.repository_url}:${var.image_tag}"
      essential = true

      portMappings = [{
        containerPort = var.container_port
        protocol      = "tcp"
      }]

      # Config is baked into the image at /etc/llm_proxy/llm_proxy.toml.
      # Secrets override the values that differ per environment.
      environment = [
        { name = "RUST_LOG", value = "info,llm_proxy=debug" },
      ]

      secrets = [
        {
          name      = "OPENAI_API_KEY"
          valueFrom = aws_secretsmanager_secret.openai_api_key.arn
        },
        {
          name      = "ANTHROPIC_API_KEY"
          valueFrom = aws_secretsmanager_secret.anthropic_api_key.arn
        },
        {
          name      = "LLM_PROXY_DATABASE_URL"
          valueFrom = aws_secretsmanager_secret.database_url.arn
        },
        {
          name      = "LLM_PROXY_LOG_DATABASE_URL"
          valueFrom = aws_secretsmanager_secret.log_database_url.arn
        },
        {
          name      = "LLM_PROXY_ADMIN_API_KEY"
          valueFrom = aws_secretsmanager_secret.admin_api_key.arn
        },
        {
          name      = "LLM_PROXY_STRIPE_SECRET_KEY"
          valueFrom = aws_secretsmanager_secret.stripe_secret_key.arn
        },
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          "awslogs-group"         = aws_cloudwatch_log_group.llm_proxy.name
          "awslogs-region"        = local.region
          "awslogs-stream-prefix" = "ecs"
        }
      }

      healthCheck = {
        command     = ["CMD-SHELL", "curl -sf http://localhost:${var.container_port}/health || exit 1"]
        interval    = 15
        timeout     = 5
        retries     = 3
        startPeriod = 10
      }
    }
  ])
}

# ── ECS Service ──────────────────────────────────────────────────────────────

resource "aws_ecs_service" "llm_proxy" {
  name            = local.name_env
  cluster         = aws_ecs_cluster.llm_proxy.id
  task_definition = aws_ecs_task_definition.llm_proxy.arn
  desired_count   = var.desired_count
  launch_type     = "FARGATE"

  network_configuration {
    subnets          = var.public_subnet_ids
    security_groups  = [aws_security_group.ecs_tasks.id]
    assign_public_ip = true
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.llm_proxy.arn
    container_name   = "llm-proxy"
    container_port   = var.container_port
  }

  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }

  deployment_maximum_percent         = 200
  deployment_minimum_healthy_percent = 100

  # Allow the service to stabilize on first deploy
  health_check_grace_period_seconds = 30

  depends_on = [aws_lb_listener.internal_http, aws_lb_listener.public_http]

  lifecycle {
    ignore_changes = [desired_count] # Let autoscaling manage this
  }
}

# ── Autoscaling ──────────────────────────────────────────────────────────────

resource "aws_appautoscaling_target" "llm_proxy" {
  max_capacity       = 10
  min_capacity       = var.desired_count
  resource_id        = "service/${aws_ecs_cluster.llm_proxy.name}/${aws_ecs_service.llm_proxy.name}"
  scalable_dimension = "ecs:service:DesiredCount"
  service_namespace  = "ecs"
}

resource "aws_appautoscaling_policy" "cpu" {
  name               = "${local.name_env}-cpu-scaling"
  policy_type        = "TargetTrackingScaling"
  resource_id        = aws_appautoscaling_target.llm_proxy.resource_id
  scalable_dimension = aws_appautoscaling_target.llm_proxy.scalable_dimension
  service_namespace  = aws_appautoscaling_target.llm_proxy.service_namespace

  target_tracking_scaling_policy_configuration {
    predefined_metric_specification {
      predefined_metric_type = "ECSServiceAverageCPUUtilization"
    }
    target_value       = 70
    scale_in_cooldown  = 300
    scale_out_cooldown = 60
  }
}
