# ── ECS Execution Role (used by ECS agent to pull images, write logs, read secrets) ─

resource "aws_iam_role" "ecs_execution" {
  name = "${local.name_env}-ecs-execution"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ecs-tasks.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "ecs_execution_base" {
  role       = aws_iam_role.ecs_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

# Allow the execution role to read our secrets
resource "aws_iam_role_policy" "ecs_execution_secrets" {
  name = "${local.name_env}-read-secrets"
  role = aws_iam_role.ecs_execution.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "secretsmanager:GetSecretValue",
      ]
      Resource = [
        aws_secretsmanager_secret.openai_api_key.arn,
        aws_secretsmanager_secret.anthropic_api_key.arn,
        aws_secretsmanager_secret.database_url.arn,
        aws_secretsmanager_secret.log_database_url.arn,
        aws_secretsmanager_secret.admin_api_key.arn,
        aws_secretsmanager_secret.stripe_secret_key.arn,
      ]
    }]
  })
}

# ── ECS Task Role (used by the running container) ────────────────────────────
# Currently minimal — expand if the proxy needs to call other AWS services.

resource "aws_iam_role" "ecs_task" {
  name = "${local.name_env}-ecs-task"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ecs-tasks.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}
