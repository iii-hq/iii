# Preview subdomain — created immediately during Phase 2. Used for the 24h staging
# validation window before the apex cutover. Removed in Phase 5 after cutover stabilizes.
resource "aws_route53_record" "preview_a" {
  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.preview_domain
  type    = "A"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

resource "aws_route53_record" "preview_aaaa" {
  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.preview_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

# Apex + www records — created via `terraform import` during Phase 4 cutover.
#
# DO NOT run `terraform apply` on these resources until Phase 4, because External-DNS
# currently owns the apex A/AAAA records. Apply order during Phase 4:
#   1. Remove iii-dev and iii-dev-www Ingresses from motia-argocd-values (PR merged + synced).
#   2. `terraform import aws_route53_record.apex_a <zone_id>_iii.dev_A`
#      `terraform import aws_route53_record.apex_aaaa <zone_id>_iii.dev_AAAA`
#      `terraform import aws_route53_record.www_a <zone_id>_www.iii.dev_A`
#      `terraform import aws_route53_record.www_aaaa <zone_id>_www.iii.dev_AAAA`
#   3. `terraform apply` — diff shows target changing from k8s NLB alias → CloudFront alias.
#      Single atomic Route53 UPSERT per record. Zero NXDOMAIN window.
#   4. Manually clean up the External-DNS `cname-iii.dev` and `cname-www.iii.dev` TXT
#      ownership records via aws CLI.
#
# See infra/terraform/website/README.md and the plan at
# ~/.claude/plans/lazy-nibbling-valiant.md (Phase 4) for the full runbook.

resource "aws_route53_record" "apex_a" {
  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.apex_domain
  type    = "A"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

resource "aws_route53_record" "apex_aaaa" {
  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.apex_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

resource "aws_route53_record" "www_a" {
  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.www_domain
  type    = "A"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}

resource "aws_route53_record" "www_aaaa" {
  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.www_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}
