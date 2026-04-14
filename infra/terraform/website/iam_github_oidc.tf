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

# ──────────────────────────────────────────────────────────────────────────
# Read-only role for `tf-plan.yml` — assumed by pull_request-triggered
# GitHub Actions jobs to run `terraform plan` and post the output as a PR
# comment. Scoped to `pull_request` sub claim so only PR jobs from this
# repo can assume it. Attached to AWS managed `ReadOnlyAccess` policy.
#
# Why a separate role from github_deploy_website:
#   - The deploy role has write actions (s3:PutObject/DeleteObject,
#     CreateInvalidation). Using it for `terraform plan` is over-permissive
#     and dangerous if someone accidentally runs `terraform apply` instead.
#   - The plan role is read-only, so even if a malicious PR modified
#     tf-plan.yml to run `terraform apply`, the apply would fail at any
#     resource modification due to IAM denial.
#
# Why `ReadOnlyAccess` (broad) instead of a narrow inline policy:
#   - Terraform plan Reads the state of every resource it manages across
#     dozens of AWS services. Enumerating each GetX/ListX/DescribeX action
#     is ~80 statements and brittle — any new resource type requires a
#     policy update.
#   - `ReadOnlyAccess` is maintained by AWS and updated automatically.
#   - The trust policy (pull_request sub from this specific repo) is the
#     primary security boundary. A malicious PR would still require code
#     review to merge.
# ──────────────────────────────────────────────────────────────────────────

data "aws_iam_policy_document" "github_tf_plan_trust" {
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
        "repo:${var.github_repo}:pull_request",
      ]
    }
  }
}

resource "aws_iam_role" "github_tf_plan" {
  name                 = "iii-infra-github-tf-plan"
  description          = "Read-only role assumed by GitHub Actions pull_request jobs to run `terraform plan` against any infra/terraform/* module in iii-hq/iii"
  assume_role_policy   = data.aws_iam_policy_document.github_tf_plan_trust.json
  max_session_duration = 3600
}

resource "aws_iam_role_policy_attachment" "github_tf_plan_readonly" {
  role       = aws_iam_role.github_tf_plan.name
  policy_arn = "arn:aws:iam::aws:policy/ReadOnlyAccess"
}
