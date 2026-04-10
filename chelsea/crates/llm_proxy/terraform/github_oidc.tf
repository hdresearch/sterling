# ── GitHub Actions OIDC → AWS IAM ────────────────────────────────────────────
# Lets the deploy-llm-proxy.yaml workflow assume an IAM role without long-lived
# access keys. GitHub's OIDC provider issues short-lived tokens scoped to the
# repo + branch.
#
# After applying, set the GitHub Actions secret:
#   AWS_DEPLOY_ROLE_ARN = <the role ARN from terraform output>

# ── OIDC Provider (one per AWS account, safe to import if it already exists) ──

resource "aws_iam_openid_connect_provider" "github" {
  url             = "https://token.actions.githubusercontent.com"
  client_id_list  = ["sts.amazonaws.com"]
  thumbprint_list = ["ffffffffffffffffffffffffffffffffffffffff"]

  lifecycle {
    # If another team already created this provider, import it:
    #   terraform import aws_iam_openid_connect_provider.github \
    #     arn:aws:iam::<ACCOUNT_ID>:oidc-provider/token.actions.githubusercontent.com
    prevent_destroy = true
  }
}

# ── Deploy Role ──────────────────────────────────────────────────────────────

resource "aws_iam_role" "github_deploy" {
  name = "${local.name_env}-github-deploy"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Federated = aws_iam_openid_connect_provider.github.arn
      }
      Action = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
        }
        StringLike = {
          # Lock down to this repo. Adjust org/repo if needed.
          "token.actions.githubusercontent.com:sub" = "repo:${var.github_repo}:*"
        }
      }
    }]
  })
}

# ── ECR Permissions (push images) ────────────────────────────────────────────

resource "aws_iam_role_policy" "github_deploy_ecr" {
  name = "ecr-push"
  role = aws_iam_role.github_deploy.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "ecr:GetAuthorizationToken",
        ]
        Resource = "*"
      },
      {
        Effect = "Allow"
        Action = [
          "ecr:BatchCheckLayerAvailability",
          "ecr:GetDownloadUrlForLayer",
          "ecr:BatchGetImage",
          "ecr:PutImage",
          "ecr:InitiateLayerUpload",
          "ecr:UploadLayerPart",
          "ecr:CompleteLayerUpload",
        ]
        Resource = aws_ecr_repository.llm_proxy.arn
      },
    ]
  })
}

# ── ECS Permissions (trigger deployments) ────────────────────────────────────

resource "aws_iam_role_policy" "github_deploy_ecs" {
  name = "ecs-deploy"
  role = aws_iam_role.github_deploy.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "ecs:UpdateService",
          "ecs:DescribeServices",
        ]
        Resource = aws_ecs_service.llm_proxy.id
      },
      {
        # DescribeServices requires cluster-level access for waiter
        Effect = "Allow"
        Action = [
          "ecs:DescribeClusters",
        ]
        Resource = aws_ecs_cluster.llm_proxy.arn
      },
    ]
  })
}
