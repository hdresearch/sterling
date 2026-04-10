output "ecr_repository_url" {
  description = "ECR repository URL — push images here"
  value       = aws_ecr_repository.llm_proxy.repository_url
}

output "alb_dns_name" {
  description = "Internal ALB DNS name (AWS-generated)"
  value       = aws_lb.internal.dns_name
}

output "public_alb_dns_name" {
  description = "Public ALB DNS name — point Cloudflare CNAME here"
  value       = aws_lb.public.dns_name
}

output "internal_url" {
  description = "Stable internal URL for the proxy"
  value       = "http://${var.internal_domain}"
}

output "public_url" {
  description = "Public URL for the proxy (behind Cloudflare)"
  value       = "https://${var.public_domain}"
}

output "ecs_cluster_name" {
  description = "ECS cluster name"
  value       = aws_ecs_cluster.llm_proxy.name
}

output "ecs_service_name" {
  description = "ECS service name"
  value       = aws_ecs_service.llm_proxy.name
}

output "log_group" {
  description = "CloudWatch log group"
  value       = aws_cloudwatch_log_group.llm_proxy.name
}

output "task_security_group_id" {
  description = "Security group ID for ECS tasks (add to RDS ingress if needed)"
  value       = aws_security_group.ecs_tasks.id
}

output "logs_db_endpoint" {
  description = "RDS endpoint for the logs database"
  value       = aws_db_instance.logs.endpoint
}

output "logs_db_identifier" {
  description = "RDS instance identifier for the logs database"
  value       = aws_db_instance.logs.identifier
}

output "github_deploy_role_arn" {
  description = "IAM role ARN for GitHub Actions — set as AWS_DEPLOY_ROLE_ARN secret"
  value       = aws_iam_role.github_deploy.arn
}
