# ── Secrets Manager ───────────────────────────────────────────────────────────
# Secrets are created here but values must be set manually (or via CI).
# ECS injects them as environment variables at task startup.

resource "aws_secretsmanager_secret" "openai_api_key" {
  name        = "${local.name_env}/openai-api-key"
  description = "OpenAI API key for llm_proxy"
}

resource "aws_secretsmanager_secret" "anthropic_api_key" {
  name        = "${local.name_env}/anthropic-api-key"
  description = "Anthropic API key for llm_proxy"
}

resource "aws_secretsmanager_secret" "database_url" {
  name        = "${local.name_env}/database-url"
  description = "Postgres connection string for billing DB (main vers RDS)"
}

resource "aws_secretsmanager_secret" "log_database_url" {
  name        = "${local.name_env}/log-database-url"
  description = "Postgres connection string for high-volume log DB"
}

resource "aws_secretsmanager_secret" "admin_api_key" {
  name        = "${local.name_env}/admin-api-key"
  description = "Admin API key for /admin endpoints"
}

resource "aws_secretsmanager_secret" "stripe_secret_key" {
  name        = "${local.name_env}/stripe-secret-key"
  description = "Stripe secret key for billing meter events and balance queries"
}
