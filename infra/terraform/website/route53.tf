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

# Apex + www records gated behind manage_apex_records / manage_www_records so the
# two subdomains can be cut over independently. See variable descriptions.

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
  count = var.manage_www_records ? 1 : 0

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
  count = var.manage_www_records ? 1 : 0

  zone_id = data.aws_route53_zone.iii_dev.zone_id
  name    = var.www_domain
  type    = "AAAA"

  alias {
    name                   = aws_cloudfront_distribution.site.domain_name
    zone_id                = aws_cloudfront_distribution.site.hosted_zone_id
    evaluate_target_health = false
  }
}
