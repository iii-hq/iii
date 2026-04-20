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
  # Must match the env name in GitHub repo settings EXACTLY — OIDC sub claim is case-sensitive.
  description = "GitHub environment scoping the deploy role."
  type        = string
  default     = "iii-website-prod"
}

variable "csp_report_only" {
  description = "Send CSP as report-only instead of enforcing."
  type        = bool
  default     = true
}

variable "manage_apex_records" {
  # Leave false until Phase 4 cutover. When false, the apex records are not managed
  # by this module (they already exist — manually created in Route53, not
  # External-DNS-owned). Flip to true after importing the existing records.
  description = "Whether Terraform manages the iii.dev apex A/AAAA Route53 records."
  type        = bool
  default     = false
}

variable "manage_www_records" {
  # Leave false until www.iii.dev has been released by External-DNS (argocd PR
  # removing the iii-dev-www Ingress has merged + synced). Flip to true after
  # importing the existing records. Decoupled from apex so the two can be
  # cut over independently to limit blast radius.
  description = "Whether Terraform manages the www.iii.dev A/AAAA Route53 records."
  type        = bool
  default     = false
}
