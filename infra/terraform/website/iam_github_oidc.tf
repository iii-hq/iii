# Reuse the existing GitHub OIDC provider if present in the account, else create one.
# Phase 0 of the runbook asks you to verify existence via
#   aws iam list-open-id-connect-providers
# If a provider for token.actions.githubusercontent.com already exists, set
# TF_VAR_github_oidc_provider_arn (or edit the data source to use it) rather than
# creating a second one.

resource "aws_iam_openid_connect_provider" "github" {
  url = "https://token.actions.githubusercontent.com"

  client_id_list = ["sts.amazonaws.com"]

  # GitHub's root CA thumbprints. AWS no longer validates these since 2023 (OIDC
  # signature verification replaced certificate validation), but the field is still
  # required by the resource schema.
  thumbprint_list = [
    "6938fd4d98bab03faadb97b34396831e3780aea1",
    "1c58a3a8518e8759bf075b76b750d4f2df264fcd",
  ]
}

data "aws_iam_policy_document" "github_trust" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRoleWithWebIdentity"]

    principals {
      type        = "Federated"
      identifiers = [aws_iam_openid_connect_provider.github.arn]
    }

    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }

    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:sub"
      values = [
        "repo:${var.github_repo}:ref:refs/heads/main",
        "repo:${var.github_repo}:environment:${var.github_environment}",
      ]
    }
  }
}

resource "aws_iam_role" "github_deploy_website" {
  name                 = "iii-website-prod-github-deploy"
  description          = "Assumed by GitHub Actions from iii-hq/iii main branch + production env to deploy the iii.dev website"
  assume_role_policy   = data.aws_iam_policy_document.github_trust.json
  max_session_duration = 3600
}

data "aws_iam_policy_document" "github_deploy_website" {
  statement {
    sid    = "ListBucket"
    effect = "Allow"
    actions = [
      "s3:ListBucket",
      "s3:GetBucketLocation",
    ]
    resources = [aws_s3_bucket.site.arn]
  }

  statement {
    sid    = "ReadWriteObjects"
    effect = "Allow"
    actions = [
      "s3:GetObject",
      "s3:PutObject",
      "s3:DeleteObject",
    ]
    resources = ["${aws_s3_bucket.site.arn}/*"]
  }

  statement {
    sid    = "CloudFrontInvalidation"
    effect = "Allow"
    actions = [
      "cloudfront:CreateInvalidation",
      "cloudfront:GetInvalidation",
      "cloudfront:ListInvalidations",
    ]
    resources = [aws_cloudfront_distribution.site.arn]
  }
}

resource "aws_iam_policy" "github_deploy_website" {
  name   = "iii-website-prod-github-deploy"
  policy = data.aws_iam_policy_document.github_deploy_website.json
}

resource "aws_iam_role_policy_attachment" "github_deploy_website" {
  role       = aws_iam_role.github_deploy_website.name
  policy_arn = aws_iam_policy.github_deploy_website.arn
}
