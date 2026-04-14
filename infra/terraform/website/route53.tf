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

# Apex + www records — GATED behind var.manage_apex_records (default false).
#
# Phase 2 (preview-only): var.manage_apex_records = false, count = 0, no resources.
# The apex `iii.dev` and `www.iii.dev` records already exist in Route53 and are
# NOT touched by this module during Phase 2. Attempting to create them without
# the gate (which is what happened on the first apply attempt) causes
# `InvalidChangeBatch: record already exists` because:
#   - apex iii.dev A/AAAA: manually created ALIAS to the k8s public NLB
#   - www.iii.dev A/AAAA:  managed by External-DNS from ingress/site/iii-dev-www
#
# Phase 4 (cutover): var.manage_apex_records = true, after:
#   1. An argocd-values PR removes the `iii-dev-www` Ingress so External-DNS
#      (upsert-only policy) stops managing www.iii.dev. Merge + ArgoCD sync.
#      The apex `iii-dev` Ingress stays — it was never External-DNS-owned.
#   2. Import the existing records into TF state (note the [0] index, because
#      the resources are count-ed):
#        terraform import 'aws_route53_record.apex_a[0]'    $ZONE_iii.dev_A
#        terraform import 'aws_route53_record.apex_aaaa[0]' $ZONE_iii.dev_AAAA
#        terraform import 'aws_route53_record.www_a[0]'     $ZONE_www.iii.dev_A
#        terraform import 'aws_route53_record.www_aaaa[0]'  $ZONE_www.iii.dev_AAAA
#   3. `terraform apply -var='manage_apex_records=true'` — diff shows each
#      record's ALIAS target changing from the k8s NLB to the CloudFront
#      distribution. Atomic single Route53 UPSERT per record. Zero NXDOMAIN
#      window.
#   4. Manually clean up the External-DNS TXT ownership record
#      `external-dns-cname-www.iii.dev` via aws CLI. (There is no
#      `cname-iii.dev` TXT — the apex was never External-DNS-owned.)
#
# See infra/terraform/website/README.md and the plan at
# ~/.claude/plans/lazy-nibbling-valiant.md (Phase 4) for the full runbook.

resource "aws_route53_record" "apex_a" {
  count = var.manage_apex_records ? 1 : 0

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
  count = var.manage_apex_records ? 1 : 0

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
  count = var.manage_apex_records ? 1 : 0

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
  count = var.manage_apex_records ? 1 : 0

  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.www_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}
