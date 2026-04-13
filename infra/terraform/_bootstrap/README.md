# Terraform Remote State Bootstrap

Creates the shared S3 bucket + DynamoDB table used as the Terraform remote state
backend for all `infra/terraform/*` modules in this repo.

## Why this module uses local state

Chicken-and-egg: the state bucket can't store its own state. Local `terraform.tfstate`
is committed to the repo on purpose so that any team member can re-apply against the
real AWS resources without losing history. **There are no secrets in this state file**
— it only describes two empty shared-infra resources (a bucket and a table).

## Resources

- `aws_s3_bucket.terraform_state` → `iii-terraform-state-prod-us-east-1`
  - Versioning on (state file history)
  - SSE-S3 (server-side encryption)
  - All public access blocked
- `aws_dynamodb_table.terraform_locks` → `iii-terraform-locks-prod`
  - PAY_PER_REQUEST billing
  - Hash key `LockID` (required by the Terraform S3 backend)

## One-time apply

```bash
cd infra/terraform/_bootstrap
terraform init
terraform apply
```

Commit the resulting `terraform.tfstate` and `.terraform.lock.hcl`.

## Re-applying later

If you need to change these resources, edit the files, run `terraform plan` + `apply`,
then commit the updated `terraform.tfstate`.

## Who uses this backend

Every other module under `infra/terraform/*` references this bucket + table in its
`backend "s3"` block. Today:

- `infra/terraform/website` — the iii.dev marketing site (S3 + CloudFront).
