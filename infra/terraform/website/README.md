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

During Phase 2 the apex/www records stayed managed by the live production setup
(apex manual, www External-DNS). They were gated out of the TF module by
`manage_apex_records = false`. Phase 4 flips that gate and brings those records
into TF state via `terraform import`.

See the full runbook in the plan. Short version:

1. Open a PR in `motia-argocd-values` removing **only** the `iii-dev-www` Ingress
   from `apps/site/manifests/iii-dev.yaml`. Leave `iii-dev`, `iii-dev-docs`, and
   `iii-dev-install` intact:
     - `iii-dev` (apex) was never External-DNS-owned; removing it does nothing
       for DNS but could affect the edge-proxy configmap, so leave it.
     - `iii-dev-docs` and `iii-dev-install` still serve docs.iii.dev and
       install.iii.dev through the k8s edge proxy (out of scope here).
   Merge and let ArgoCD sync. Verify with `kubectl get ingress -n site` —
   `iii-dev-www` should be gone, the other three remain.

2. Confirm External-DNS has stopped managing `www.iii.dev`:
   ```bash
   kubectl logs -n external-dns deploy/external-dns --tail 100 | grep www.iii.dev
   ```

3. Import the existing Route53 records into Terraform state. Note the `[0]`
   index — the resources are `count = var.manage_apex_records ? 1 : 0`:
   ```bash
   ZONE=$(aws --profile motia-prod route53 list-hosted-zones \
     --query "HostedZones[?Name=='iii.dev.'].Id" --output text | awk -F/ '{print $NF}')

   terraform import 'aws_route53_record.apex_a[0]'    "${ZONE}_iii.dev_A"
   terraform import 'aws_route53_record.apex_aaaa[0]' "${ZONE}_iii.dev_AAAA"
   terraform import 'aws_route53_record.www_a[0]'     "${ZONE}_www.iii.dev_A"
   terraform import 'aws_route53_record.www_aaaa[0]'  "${ZONE}_www.iii.dev_AAAA"
   ```
   If you forget `-var='manage_apex_records=true'` here, the import will store
   a resource that's not in config and the next plan will try to delete it.
   Set the var in `terraform.tfvars` or export `TF_VAR_manage_apex_records=true`
   before running the imports.

4. `terraform plan -var='manage_apex_records=true'` — expect exactly four
   in-place updates, each changing the ALIAS target from the k8s NLB
   (`abfa…elb.us-east-1.amazonaws.com`) to the CloudFront distribution
   (`d***.cloudfront.net`). Any destroy/create or other drift → STOP.

5. `terraform apply -var='manage_apex_records=true'` — single atomic Route53
   UPSERT per record. No NXDOMAIN window.

6. Flip `csp_report_only` to `false` (can be combined with step 5):
   ```bash
   terraform apply -var='manage_apex_records=true' -var='csp_report_only=false'
   ```

7. Clean up the orphaned External-DNS TXT ownership record for www via aws CLI:
   ```bash
   aws --profile motia-prod route53 change-resource-record-sets \
     --hosted-zone-id $ZONE \
     --change-batch file:///tmp/delete-www-txt.json
   ```
   (Fetch the exact TXT value first with `list-resource-record-sets
   --query "ResourceRecordSets[?Name=='external-dns-cname-www.iii.dev.']"`.)
   There is no `cname-iii.dev` TXT because the apex was never External-DNS-owned.

### Phase 5 — remove preview

After 48–72h of clean metrics:

1. Remove the `preview_*` Route53 records and the `preview_domain` from `acm.tf`'s
   SAN list (or set `preview_domain` to empty and make the records conditional).
2. `terraform apply -var='manage_apex_records=true'` — ACM re-issues the cert with two SANs instead of three.
3. Delete the explicit CAA record at `iii-preview.iii.dev` (was added as a workaround for the `*.iii.dev` wildcard CNAME intercepting CAA lookup; no longer needed once the preview subdomain goes away).
4. Delete the Vercel iii-web project.

## Rollback

If Phase 4 goes bad, rollback differs between apex (manually-owned) and www
(External-DNS-owned):

**Apex `iii.dev`** — manually owned in Route53, not in argocd-values.

```bash
ZONE=Z05516132AI1ZGB3NLC6D
aws --profile motia-prod route53 change-resource-record-sets \
  --hosted-zone-id $ZONE \
  --change-batch '{
    "Changes": [{
      "Action": "UPSERT",
      "ResourceRecordSet": {
        "Name": "iii.dev.",
        "Type": "A",
        "AliasTarget": {
          "HostedZoneId": "Z26RNL4JYFTOTI",
          "DNSName": "abfa1050d1bf345f2ad3abef639593d8-62c8780bba490615.elb.us-east-1.amazonaws.com.",
          "EvaluateTargetHealth": false
        }
      }
    }]
  }'
# Repeat for AAAA. Then remove from TF state so future applies don't fight:
terraform state rm 'aws_route53_record.apex_a[0]' 'aws_route53_record.apex_aaaa[0]'
```

**www `www.iii.dev`** — External-DNS originally owned it.

1. Revert the `motia-argocd-values` PR that removed the `iii-dev-www` Ingress.
   ArgoCD re-applies. External-DNS (upsert-only) re-upserts the A/AAAA records
   pointing at the k8s NLB, overwriting the TF-managed CloudFront ALIAS.
2. `terraform state rm 'aws_route53_record.www_a[0]' 'aws_route53_record.www_aaaa[0]'`

TTL on ALIAS records is 0 (edge-resolved), so propagation is ~immediate.

**Emergency fallback:** re-attach the `iii.dev` domain in the Vercel dashboard
and manually point Route53 at Vercel's edge via a CNAME. Bypasses both k8s and
CloudFront entirely.

## Related files

- `cloudfront_functions/redirects.js` — CloudFront Function source (must be <10KB)
- `cloudfront_functions/redirects.test.js` — `node --test` unit tests for the function
- `.github/workflows/deploy-website.yml` — builds and deploys `website/` on push to main
- `.github/workflows/smoke.yml` — post-deploy smoke checks
- `.github/workflows/tf-plan.yml` — PR-triggered `terraform plan` comment
- `.github/workflows/cloudfront-functions-test.yml` — runs the CF function unit tests on PR
