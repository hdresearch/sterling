variable "environment" {
  description = "Deployment environment (staging, production)"
  type        = string
  default     = "production"
}

# ── Networking ────────────────────────────────────────────────────────────────

variable "vpc_id" {
  description = "VPC ID where the service runs"
  type        = string
}

variable "private_subnet_ids" {
  description = "Private subnet IDs for ECS tasks (need NAT gateway for outbound)"
  type        = list(string)
}

variable "public_subnet_ids" {
  description = "Subnet IDs for the internal ALB (must span ≥2 AZs)"
  type        = list(string)
}

variable "internal_zone_id" {
  description = "Route53 private hosted zone ID for internal DNS (e.g. internal.vers.sh)"
  type        = string
  default     = ""
}

variable "internal_domain" {
  description = "Internal DNS name for the proxy (e.g. tokens.internal.vers.sh)"
  type        = string
  default     = "tokens.internal.vers.sh"
}

variable "public_domain" {
  description = "Public DNS name for the proxy (e.g. tokens.vers.sh). DNS managed externally (Cloudflare)."
  type        = string
  default     = "tokens.vers.sh"
}

# ── Database ──────────────────────────────────────────────────────────────────

variable "database_security_group_id" {
  description = "Security group ID of the RDS instance (to allow ingress from ECS tasks)"
  type        = string
  default     = ""
}

# ── Logs Database (separate RDS instance) ─────────────────────────────────────

variable "logs_db_instance_class" {
  description = "RDS instance class for the logs database"
  type        = string
  default     = "db.t4g.medium"
}

variable "logs_db_allocated_storage" {
  description = "Initial storage in GB for the logs database"
  type        = number
  default     = 20
}

variable "logs_db_max_allocated_storage" {
  description = "Max autoscaled storage in GB for the logs database"
  type        = number
  default     = 100
}

# ── Service sizing ────────────────────────────────────────────────────────────

variable "cpu" {
  description = "Fargate task CPU units (256 = 0.25 vCPU)"
  type        = number
  default     = 512
}

variable "memory" {
  description = "Fargate task memory in MiB"
  type        = number
  default     = 1024
}

variable "desired_count" {
  description = "Number of ECS tasks to run"
  type        = number
  default     = 2
}

variable "container_port" {
  description = "Port the llm_proxy container listens on"
  type        = number
  default     = 8090
}

# ── Image ─────────────────────────────────────────────────────────────────────

variable "image_tag" {
  description = "Container image tag to deploy"
  type        = string
  default     = "latest"
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days"
  type        = number
  default     = 30
}

# ── GitHub Actions ─────────────────────────────────────────────────────────────

variable "github_repo" {
  description = "GitHub org/repo for OIDC trust (e.g. hdresearch/chelsea)"
  type        = string
  default     = "hdresearch/chelsea"
}

variable "tags" {
  description = "Tags applied to all resources"
  type        = map(string)
  default = {
    Service   = "llm-proxy"
    ManagedBy = "terraform"
  }
}
