resource "aws_s3_bucket" "site" {
  bucket = var.bucket_name
}

resource "aws_s3_bucket_versioning" "site" {
  bucket = aws_s3_bucket.site.id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "site" {
  bucket = aws_s3_bucket.site.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "site" {
  bucket = aws_s3_bucket.site.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

data "aws_iam_policy_document" "site_bucket" {
  # Allow CloudFront (scoped to our distribution via OAC + SourceArn) to fetch
  # individual objects from the bucket.
  statement {
    sid    = "AllowCloudFrontOACGetObject"
    effect = "Allow"

    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }

    actions = ["s3:GetObject"]

    resources = ["${aws_s3_bucket.site.arn}/*"]

    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.site.arn]
    }
  }

  # Allow CloudFront to call ListBucket on the bucket (not objects). Without
  # this, S3 returns 403 for missing objects instead of 404, because S3 uses
  # ListBucket permission to distinguish "key doesn't exist" from "access
  # denied." This does NOT expose a public LIST endpoint — CloudFront only
  # ever makes GetObject calls for specific keys at the viewer's request. The
  # ListBucket permission just changes the internal S3 error code from 403 to
  # 404, which is what end users correctly see for missing paths like
  # /missing.jpg.
  statement {
    sid    = "AllowCloudFrontOACListBucketForCorrect404"
    effect = "Allow"

    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }

    actions = ["s3:ListBucket"]

    resources = [aws_s3_bucket.site.arn]

    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.site.arn]
    }
  }
}

resource "aws_s3_bucket_policy" "site" {
  bucket = aws_s3_bucket.site.id
  policy = data.aws_iam_policy_document.site_bucket.json
}
