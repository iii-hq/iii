terraform {
  backend "s3" {
    bucket         = "iii-terraform-state-prod-us-east-1"
    key            = "website/terraform.tfstate"
    region         = "us-east-1"
    dynamodb_table = "iii-terraform-locks-prod"
    encrypt        = true
  }
}

provider "aws" {
  region = "us-east-1"

  # Credentials resolved via the standard AWS SDK credential chain:
  #   - GitHub Actions: env vars from aws-actions/configure-aws-credentials (OIDC)
  #   - Local dev:       export AWS_PROFILE=motia-prod before running terraform
  # Do NOT hardcode `profile = "motia-prod"` here — it would override OIDC env
  # credentials in CI and fail with "failed to get shared config profile".

  default_tags {
    tags = {
      Project     = "iii"
      Environment = "prod"
      Service     = "website"
      Module      = "infra/terraform/website"
      ManagedBy   = "terraform"
    }
  }
}

data "aws_caller_identity" "current" {}
data "aws_route53_zone" "iii_dev" {
  name         = "iii.dev."
  private_zone = false
}
