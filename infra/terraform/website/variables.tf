variable "bucket_name" {
  description = "Name of the S3 bucket that stores the iii.dev website build output"
  type        = string
  default     = "iii-website-prod-us-east-1"
}

variable "apex_domain" {
  description = "Apex domain for the marketing site"
  type        = string
  default     = "iii.dev"
}

variable "www_domain" {
  description = "www subdomain (301s to apex via CloudFront Function)"
  type        = string
  default     = "www.iii.dev"
}

variable "preview_domain" {
  description = "Preview subdomain used for the 24h staging window before apex cutover. Remove after cutover stabilizes."
  type        = string
  default     = "iii-preview.iii.dev"
}

variable "docs_domain" {
  description = "Docs subdomain that /docs/* is 301'd to"
  type        = string
  default     = "docs.iii.dev"
}

variable "search_api_origin" {
  description = "Custom origin hostname that /api/search* is proxied to (temporarily, until docs migration)"
  type        = string
  default     = "iii-docs.vercel.app"
}

variable "alarm_email" {
  description = "Email address that receives SNS notifications for production alarms"
  type        = string
}

variable "price_class" {
  description = "CloudFront price class"
  type        = string
  default     = "PriceClass_All"
}

variable "github_repo" {
  description = "GitHub repo allowed to assume the deploy role via OIDC"
  type        = string
  default     = "iii-hq/iii"
}

variable "github_environment" {
  description = <<-EOT
    GitHub environment the deploy role is scoped to. Must match the environment
    name used in `.github/workflows/deploy-website.yml`. Scoped to an isolated
    environment (not the shared `Production`) so the AWS deploy secrets are not
    exposed to unrelated workflows.

    IMPORTANT: GitHub's OIDC sub claim uses the stored case of the environment
    name, and IAM trust policy `StringEquals` is case-sensitive. Keep this value
    in exact sync with the environment name in GitHub repo settings.
  EOT
  type        = string
  default     = "iii-website-prod"
}

variable "csp_report_only" {
  description = "If true, CSP is sent as Content-Security-Policy-Report-Only (no enforcement). Flip to false after the 24h staging window."
  type        = bool
  default     = true
}

variable "manage_apex_records" {
  description = <<-EOT
    PHASE GATE — leave at `false` during Phase 2 (preview-only).

    When `false`, Terraform does NOT create Route53 records for `iii.dev` or
    `www.iii.dev`. This is correct during Phase 2 because those records already
    exist in the zone (apex is manually created, www is managed by External-DNS
    via the `iii-dev-www` Ingress in motia-argocd-values). Creating them from
    this module would fail with `InvalidChangeBatch: record already exists`.

    Flip to `true` ONLY during Phase 4 (apex cutover), after:
      1. An argocd-values PR has removed the `iii-dev-www` Ingress (so
         External-DNS stops managing `www.iii.dev`).
      2. You have run `terraform import` for the four records:
           terraform import 'aws_route53_record.apex_a[0]'    $ZONE_iii.dev_A
           terraform import 'aws_route53_record.apex_aaaa[0]' $ZONE_iii.dev_AAAA
           terraform import 'aws_route53_record.www_a[0]'     $ZONE_www.iii.dev_A
           terraform import 'aws_route53_record.www_aaaa[0]'  $ZONE_www.iii.dev_AAAA

    `terraform apply -var='manage_apex_records=true'` will then show an in-place
    update swapping each record's ALIAS target from the k8s NLB to the
    CloudFront distribution. Atomic single Route53 UPSERT per record.

    See infra/terraform/website/README.md for the full Phase 4 runbook.
  EOT
  type        = bool
  default     = false
}
