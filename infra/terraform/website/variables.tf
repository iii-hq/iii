variable "bucket_name" {
  description = "Name of the S3 bucket that stores the iii.dev website build output"
  type        = string
  default     = "motia-prod-iii-website-us-east-1"
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
  # Intentionally no default — set via terraform.tfvars or TF_VAR_alarm_email.
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
  description = "GitHub environment the deploy role is scoped to"
  type        = string
  default     = "production"
}

variable "csp_report_only" {
  description = "If true, CSP is sent as Content-Security-Policy-Report-Only (no enforcement). Flip to false after the 24h staging window."
  type        = bool
  default     = true
}
