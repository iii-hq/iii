output "bucket_name" {
  description = "Name of the S3 bucket hosting the website build output"
  value       = aws_s3_bucket.site.bucket
}

output "bucket_arn" {
  description = "ARN of the S3 bucket"
  value       = aws_s3_bucket.site.arn
}

output "distribution_id" {
  description = "CloudFront distribution ID — set as GitHub repo variable CF_DISTRIBUTION_ID"
  value       = aws_cloudfront_distribution.site.id
}

output "distribution_domain_name" {
  description = "CloudFront distribution domain (dXXXXX.cloudfront.net) — target of Route53 ALIAS records"
  value       = aws_cloudfront_distribution.site.domain_name
}

output "distribution_arn" {
  description = "CloudFront distribution ARN"
  value       = aws_cloudfront_distribution.site.arn
}

output "cert_arn" {
  description = "ACM certificate ARN for iii.dev, www.iii.dev, iii-preview.iii.dev"
  value       = aws_acm_certificate.site.arn
}

output "github_deploy_role_arn" {
  description = "IAM role ARN assumed by GitHub Actions from iii-hq/iii main branch + production env — set as GitHub secret AWS_DEPLOY_ROLE_ARN"
  value       = aws_iam_role.github_deploy_website.arn
}

output "github_tf_plan_role_arn" {
  description = "Read-only IAM role ARN assumed by pull_request-triggered GitHub Actions jobs for `terraform plan` — set as repo-level GitHub secret AWS_READONLY_ROLE_ARN"
  value       = aws_iam_role.github_tf_plan.arn
}

output "sns_alarms_topic_arn" {
  description = "SNS topic ARN that receives production alarms"
  value       = aws_sns_topic.alarms.arn
}
