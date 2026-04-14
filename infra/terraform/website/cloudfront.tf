locals {
  # 'unsafe-inline' required for inline GTM/Tailwind scripts in index.html.
  # 'unsafe-eval' required by the Tailwind CDN runtime (`new Function()`).
  csp = join("; ", [
    "default-src 'self'",
    "script-src 'self' 'unsafe-inline' 'unsafe-eval' https://www.googletagmanager.com https://cdn.tailwindcss.com https://esm.sh https://cdn.cr-relay.com",
    "style-src 'self' 'unsafe-inline' https://cdn.tailwindcss.com",
    "img-src 'self' data: https:",
    "font-src 'self' data:",
    "connect-src 'self' https://www.google-analytics.com https://analytics.google.com https://www.googletagmanager.com https://api.cr-relay.com https://api.mailmodo.com https://${var.search_api_origin}",
    "frame-src https://www.googletagmanager.com",
    "form-action 'self' https://api.mailmodo.com",
    "object-src 'none'",
    "base-uri 'self'",
    "frame-ancestors 'none'",
    "upgrade-insecure-requests",
  ])
}

resource "aws_cloudfront_origin_access_control" "site" {
  name                              = "iii-website-prod-oac"
  description                       = "OAC for iii.dev website S3 origin"
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

resource "aws_cloudfront_function" "redirects" {
  name    = "iii-website-prod-redirects"
  runtime = "cloudfront-js-2.0"
  comment = "viewer-request: www->apex, /docs->docs.iii.dev, /llms.txt redirect, SPA fallback"
  publish = true
  code    = file("${path.module}/cloudfront_functions/redirects.js")
}

resource "aws_cloudfront_response_headers_policy" "site" {
  name    = "iii-website-prod-security-headers"
  comment = "HSTS + CSP + security headers for iii.dev"

  security_headers_config {
    content_type_options {
      override = true
    }

    frame_options {
      frame_option = "DENY"
      override     = true
    }

    referrer_policy {
      referrer_policy = "strict-origin-when-cross-origin"
      override        = true
    }

    # preload is a ONE-WAY DOOR — would apply forever to cloud.iii.dev and any
    # *.iii.dev wildcard subdomain. Audit every subdomain first.
    strict_transport_security {
      access_control_max_age_sec = 31536000
      include_subdomains         = true
      preload                    = false
      override                   = true
    }

    xss_protection {
      mode_block = true
      protection = true
      override   = true
    }
  }

  custom_headers_config {
    items {
      header   = var.csp_report_only ? "Content-Security-Policy-Report-Only" : "Content-Security-Policy"
      value    = local.csp
      override = true
    }
  }
}

locals {
  cache_policy_optimized_id    = "658327ea-f89d-4fab-a63d-7e88639e58f6" # CachingOptimized
  cache_policy_disabled_id     = "4135ea2d-6df8-44a3-9df3-4b5a84be39ad" # CachingDisabled
  origin_request_cors_s3_id    = "88a5eaf4-2fd4-4709-b370-b4c650ea3fcf" # CORS-S3Origin
  origin_request_all_viewer_id = "216adef6-5c7f-47e4-b989-5492eafa07d3" # AllViewer
}

resource "aws_cloudfront_distribution" "site" {
  enabled             = true
  is_ipv6_enabled     = true
  comment             = "iii.dev marketing site (${var.apex_domain})"
  default_root_object = "index.html"
  http_version        = "http2and3"
  price_class         = var.price_class

  aliases = [
    var.apex_domain,
    var.www_domain,
    var.preview_domain,
  ]

  origin {
    origin_id                = "s3-site"
    domain_name              = aws_s3_bucket.site.bucket_regional_domain_name
    origin_access_control_id = aws_cloudfront_origin_access_control.site.id
  }

  origin {
    origin_id   = "search-api"
    domain_name = var.search_api_origin

    custom_origin_config {
      http_port                = 80
      https_port               = 443
      origin_protocol_policy   = "https-only"
      origin_ssl_protocols     = ["TLSv1.2"]
      origin_keepalive_timeout = 5
      origin_read_timeout      = 30
    }
  }

  default_cache_behavior {
    target_origin_id       = "s3-site"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD", "OPTIONS"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true

    cache_policy_id            = local.cache_policy_optimized_id
    origin_request_policy_id   = local.origin_request_cors_s3_id
    response_headers_policy_id = aws_cloudfront_response_headers_policy.site.id

    function_association {
      event_type   = "viewer-request"
      function_arn = aws_cloudfront_function.redirects.arn
    }
  }

  ordered_cache_behavior {
    path_pattern           = "/api/search*"
    target_origin_id       = "search-api"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD", "OPTIONS", "PUT", "POST", "PATCH", "DELETE"]
    cached_methods         = ["GET", "HEAD"]
    compress               = true

    cache_policy_id            = local.cache_policy_disabled_id
    origin_request_policy_id   = local.origin_request_all_viewer_id
    response_headers_policy_id = aws_cloudfront_response_headers_policy.site.id

    # No function_association: SPA fallback must not rewrite /api/search responses.
  }

  viewer_certificate {
    acm_certificate_arn      = aws_acm_certificate_validation.site.certificate_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }
}
