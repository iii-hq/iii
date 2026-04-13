terraform {
  backend "s3" {
    bucket         = "motia-prod-terraform-state-us-east-1"
    key            = "website/terraform.tfstate"
    region         = "us-east-1"
    dynamodb_table = "motia-prod-terraform-locks"
    encrypt        = true
  }
}

provider "aws" {
  region  = "us-east-1"
  profile = "motia-prod"

  default_tags {
    tags = {
      Project     = "iii"
      Environment = "prod"
      Module      = "infra/terraform/website"
      ManagedBy   = "terraform"
      Service     = "iii-dev-website"
    }
  }
}

data "aws_caller_identity" "current" {}
data "aws_route53_zone" "iii_dev" {
  name         = "iii.dev."
  private_zone = false
}
