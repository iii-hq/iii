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
