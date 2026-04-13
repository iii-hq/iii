resource "aws_sns_topic" "alarms" {
  name = "iii-website-prod-alarms"
}

resource "aws_sns_topic_subscription" "email" {
  topic_arn = aws_sns_topic.alarms.arn
  protocol  = "email"
  endpoint  = var.alarm_email
  # AWS sends a confirmation email to `alarm_email` after first apply. The
  # subscription stays in "PendingConfirmation" until someone clicks the link.
  # Documented in infra/terraform/website/README.md.
}

# Catches bad deploys, CF Function JS errors, and origin (S3 or search API) 5xx bursts.
resource "aws_cloudwatch_metric_alarm" "cf_5xx_rate" {
  alarm_name          = "iii-website-prod-cf-5xx-rate"
  alarm_description   = "CloudFront 5xxErrorRate above 1% for the iii.dev distribution"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 2
  datapoints_to_alarm = 2
  threshold           = 1.0
  treat_missing_data  = "notBreaching"

  metric_name = "5xxErrorRate"
  namespace   = "AWS/CloudFront"
  statistic   = "Average"
  period      = 60
  unit        = "Percent"

  dimensions = {
    DistributionId = aws_cloudfront_distribution.site.id
    Region         = "Global"
  }

  alarm_actions = [aws_sns_topic.alarms.arn]
  ok_actions    = [aws_sns_topic.alarms.arn]
}

# Catches the case where ACM auto-renewal fails and the cert is about to expire.
# ACM publishes DaysToExpiry per certificate in us-east-1 for CloudFront certs.
resource "aws_cloudwatch_metric_alarm" "acm_days_to_expiry" {
  alarm_name          = "iii-website-prod-acm-days-to-expiry"
  alarm_description   = "ACM certificate for iii.dev is within 30 days of expiring and has not renewed"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 1
  threshold           = 30
  treat_missing_data  = "breaching"

  metric_name = "DaysToExpiry"
  namespace   = "AWS/CertificateManager"
  statistic   = "Minimum"
  period      = 86400 # 1 day

  dimensions = {
    CertificateArn = aws_acm_certificate.site.arn
  }

  alarm_actions = [aws_sns_topic.alarms.arn]
}
