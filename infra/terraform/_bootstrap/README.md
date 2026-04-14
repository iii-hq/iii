# Terraform Remote State Bootstrap

Creates the shared S3 bucket + DynamoDB table used as the Terraform remote state
backend for all `infra/terraform/*` modules in this repo.

## State storage

This module uses **remote state** stored in the very bucket it creates:

- Bucket: `iii-terraform-state-prod-us-east-1`
- Key: `_bootstrap/terraform.tfstate`
- Lock table: `iii-terraform-locks-prod`

The state file is **NOT committed to git**. It lives exclusively in S3 (SSE-S3,
versioning enabled, public access blocked). This avoids leaking AWS account IDs,
resource ARNs, and any future sensitive attributes via the public repo.

## Resources

- `aws_s3_bucket.terraform_state` → `iii-terraform-state-prod-us-east-1`
  - Versioning on (state file history)
  - SSE-S3 (server-side encryption)
  - All public access blocked
- `aws_dynamodb_table.terraform_locks` → `iii-terraform-locks-prod`
  - PAY_PER_REQUEST billing
  - Hash key `LockID` (required by the Terraform S3 backend)

## Re-applying (bucket + table already exist)

Normal case — someone needs to change a tag, add a bucket lifecycle rule, etc.:

```bash
cd infra/terraform/_bootstrap
export AWS_PROFILE=motia-prod
terraform init
terraform plan
terraform apply
```

The state loads from S3 automatically. No local state file required.

## Re-bootstrap from scratch (rare, destructive)

If the bucket and/or lock table have been deleted and need to be recreated,
the `backend "s3"` block in `main.tf` becomes a chicken-and-egg problem: you
can't init against a bucket that doesn't exist. Procedure:

1. **Temporarily comment out** the `terraform { backend "s3" { ... } }` block
   in `main.tf`. This drops the module back to local state for one apply.
2. Run the initial apply:
   ```bash
   terraform init
   terraform apply
   ```
   This creates the bucket and the lock table. A local `terraform.tfstate`
   file is written.
3. **Uncomment** the `backend "s3"` block.
4. Migrate the local state into the now-existing bucket:
   ```bash
   terraform init -migrate-state
   # Prompt: "Do you want to copy existing state to the new backend?" → yes
   ```
5. Delete the local `terraform.tfstate` (and `terraform.tfstate.backup`) files.
   **Do NOT commit them.** They are listed in `infra/terraform/.gitignore`.

## Who uses this backend

Every other module under `infra/terraform/*` references this bucket + table in
its `backend "s3"` block. Today:

- `infra/terraform/website` — iii.dev marketing site (S3 + CloudFront). Key:
  `website/terraform.tfstate`.

## Security notes

- The state file is world-accessible to anyone with `s3:GetObject` on the bucket.
  The bucket policy and public access block prevent anonymous access. Only
  principals with explicit grants on the bucket can read it.
- **Never add resources to this module that have sensitive attributes** (DB
  passwords, API keys, TLS private keys, secret values). Even though the state
  is in a private bucket, minimizing the blast radius of an accidental leak is
  part of keeping the bootstrap module simple. Stick to infrastructure plumbing
  that doesn't carry application secrets.
- The state bucket name and account ID can be reconstructed from the `backend "s3"`
  block in `main.tf` (which IS committed). That's expected and fine — AWS does
  not consider account IDs or bucket names to be secrets.
