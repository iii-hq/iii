# Terraform: iii.dev website (S3 + CloudFront)

This module owns the AWS infrastructure for serving `iii.dev` (and `www.iii.dev`)
off CloudFront + S3 in the `motia-prod` account (`600627348446`, `us-east-1`).

Context and rationale: see the full plan at
`~/.claude/plans/lazy-nibbling-valiant.md` in the reviewer's workspace, or read
`website/README.md` after the migration lands.

## Resources created

| Resource | Purpose |
|---|---|
| `aws_s3_bucket.site` | Private bucket holding the Vite build output |
| `aws_s3_bucket_policy.site` | Allows CloudFront OAC only (no public access) |
| `aws_cloudfront_origin_access_control.site` | OAC (modern replacement for OAI) |
| `aws_cloudfront_function.redirects` | viewer-request: www→apex 301, /docs→docs.iii.dev 301, /llms.txt redirect, SPA fallback |
| `aws_cloudfront_response_headers_policy.site` | HSTS, CSP, X-Frame-Options, Referrer-Policy, X-Content-Type-Options |
| `aws_cloudfront_distribution.site` | The CDN edge. SANs: iii.dev, www.iii.dev, iii-preview.iii.dev |
| `aws_acm_certificate.site` | TLS cert with DNS validation |
| `aws_route53_record.preview_*` | ALIAS records for the 24h staging window |
| `aws_route53_record.apex_*` / `www_*` | Defined but imported in Phase 4 (see `route53.tf`) |
| `aws_iam_openid_connect_provider.github` | GitHub Actions OIDC provider |
| `aws_iam_role.github_deploy_website` | Deploy role scoped to iii-hq/iii main + production env |
| `aws_sns_topic.alarms` + `aws_cloudwatch_metric_alarm.*` | 5xx rate alarm + ACM cert expiry alarm |

## Variables

`alarm_email` has no default — set it via a `terraform.tfvars` (gitignored) or
`TF_VAR_alarm_email=...` in your shell before applying.

```hcl
# infra/terraform/website/terraform.tfvars  (gitignored)
alarm_email = "you@motia.dev"
```

Other variables have sensible defaults. See `variables.tf` for the full list.

## Staging → cutover workflow

### Phase 2 — first apply (preview only)

```bash
cd infra/terraform/website
terraform init
terraform plan      # review carefully
terraform apply     # creates S3, CF distro, ACM cert, 3 SANs, preview Route53 records
```

At this point:
- `iii-preview.iii.dev` serves from CloudFront.
- The apex `iii.dev` record is still owned by External-DNS and untouched.
- `iii.dev` and `www.iii.dev` are in the cert SANs but there are no Route53 records
  for them yet — the TF resources in `route53.tf` (`apex_*`, `www_*`) are defined
  but you must NOT apply them yet. If Terraform tries to create them now, it will
  collide with the existing External-DNS-owned records.

Confirm the SNS email subscription link in your inbox.

Upload a placeholder file to test end-to-end:

```bash
aws --profile motia-prod s3 cp /tmp/index.html s3://iii-website-prod-us-east-1/index.html
curl -I https://iii-preview.iii.dev/
```

### Phase 4 — apex cutover (user-visible)

See the full runbook in the plan. Short version:

1. Open a PR in `motia-argocd-values` removing the `iii-dev` and `iii-dev-www`
   Ingresses from `apps/site/manifests/iii-dev.yaml`. Merge and let ArgoCD sync.
2. Confirm External-DNS has stopped managing those hosts (tail its logs).
3. Import the existing Route53 records into Terraform state:
   ```bash
   ZONE=$(aws --profile motia-prod route53 list-hosted-zones \
     --query "HostedZones[?Name=='iii.dev.'].Id" --output text | awk -F/ '{print $NF}')
   terraform import aws_route53_record.apex_a   ${ZONE}_iii.dev_A
   terraform import aws_route53_record.apex_aaaa ${ZONE}_iii.dev_AAAA
   terraform import aws_route53_record.www_a    ${ZONE}_www.iii.dev_A
   terraform import aws_route53_record.www_aaaa ${ZONE}_www.iii.dev_AAAA
   ```
4. `terraform plan` — expect exactly four target changes (each ALIAS target moves
   from the k8s NLB to the CloudFront distribution). Any delete/recreate → STOP.
5. `terraform apply` — single atomic Route53 UPSERT per record. No NXDOMAIN window.
6. Flip `csp_report_only` to `false` in the plan and apply (optional, can also be
   done at the same time as step 5):
   ```bash
   terraform apply -var 'csp_report_only=false'
   ```
7. Clean up orphaned External-DNS TXT ownership records via aws CLI.

### Phase 5 — remove preview

After 48–72h of clean metrics:

1. Remove the `preview_*` Route53 records and the `preview_domain` from `acm.tf`'s
   SAN list (or set `preview_domain` to empty and make the records conditional).
2. `terraform apply` — ACM re-issues the cert with two SANs instead of three.
3. Delete the Vercel iii-web project.

## Rollback

If Phase 4 goes bad:

```bash
# Target-destroy only the apex records — this removes them from Route53, letting
# External-DNS re-create the old records on its next reconcile loop (30s-ish).
terraform apply \
  -target='aws_route53_record.apex_a' \
  -target='aws_route53_record.apex_aaaa' \
  -target='aws_route53_record.www_a' \
  -target='aws_route53_record.www_aaaa' \
  -destroy
```

Then revert the `motia-argocd-values` PR that removed the Ingresses. ArgoCD
re-applies them, External-DNS re-upserts, and `iii.dev` is back on the k8s
edge proxy serving Vercel content.

Emergency fallback: re-attach the `iii.dev` domain in the Vercel dashboard and
manually point Route53 at Vercel's edge via a CNAME.

## Related files

- `cloudfront_functions/redirects.js` — CloudFront Function source (must be <10KB)
- `cloudfront_functions/redirects.test.js` — `node --test` unit tests for the function
- `.github/workflows/deploy-website.yml` — builds and deploys `website/` on push to main
- `.github/workflows/smoke.yml` — post-deploy smoke checks
- `.github/workflows/tf-plan.yml` — PR-triggered `terraform plan` comment
- `.github/workflows/cloudfront-functions-test.yml` — runs the CF function unit tests on PR
