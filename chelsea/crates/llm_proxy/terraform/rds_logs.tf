# ── RDS Instance for LLM Proxy Logs ──────────────────────────────────────────
# Separate small Postgres instance for high-volume, append-only log data
# (spend_logs, request_logs). Keeps write-heavy log traffic off the main vers DB.

resource "aws_db_subnet_group" "logs" {
  name       = "${local.name_env}-logs"
  subnet_ids = var.private_subnet_ids

  tags = { Name = "${local.name_env}-logs" }
}

resource "aws_security_group" "logs_db" {
  name        = "${local.name_env}-logs-db"
  description = "RDS for llm_proxy logs"
  vpc_id      = var.vpc_id

  # Only ECS tasks can reach this DB
  ingress {
    description     = "Postgres from ECS tasks"
    from_port       = 5432
    to_port         = 5432
    protocol        = "tcp"
    security_groups = [aws_security_group.ecs_tasks.id]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_db_instance" "logs" {
  identifier = "${local.name_env}-logs"

  engine         = "postgres"
  engine_version = "17.5"
  instance_class = var.logs_db_instance_class

  allocated_storage     = var.logs_db_allocated_storage
  max_allocated_storage = var.logs_db_max_allocated_storage
  storage_type          = "gp3"
  storage_encrypted     = true

  db_name  = "llm_proxy_logs"
  username = "llm_proxy"
  password = random_password.logs_db.result

  db_subnet_group_name   = aws_db_subnet_group.logs.name
  vpc_security_group_ids = [aws_security_group.logs_db.id]

  multi_az            = var.environment == "production"
  publicly_accessible = false

  backup_retention_period = 7
  backup_window           = "03:00-04:00"
  maintenance_window      = "sun:04:30-sun:05:30"

  # Performance Insights (free tier covers t4g)
  performance_insights_enabled          = true
  performance_insights_retention_period = 7

  # Allow Terraform to destroy (set to true once you're confident)
  deletion_protection       = var.environment == "production"
  skip_final_snapshot       = var.environment != "production"
  final_snapshot_identifier = var.environment == "production" ? "${local.name_env}-logs-final" : null

  tags = { Name = "${local.name_env}-logs" }
}

# ── Generate a random password and store it in Secrets Manager ───────────────

resource "random_password" "logs_db" {
  length  = 32
  special = false # Avoid URL-encoding headaches in connection strings
}

# Store the full connection string as the log_database_url secret
resource "aws_secretsmanager_secret_version" "log_database_url" {
  secret_id     = aws_secretsmanager_secret.log_database_url.id
  secret_string = "postgres://${aws_db_instance.logs.username}:${random_password.logs_db.result}@${aws_db_instance.logs.endpoint}/${aws_db_instance.logs.db_name}"
}
