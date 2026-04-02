# Deploy Flow - Motia Framework

This document describes the complete automated deploy flow for the Motia framework.

## Overview

The deploy flow is divided into 3 main stages:

1. **Build and Pre-Release** (`deploy.yml`)
2. **E2E Tests** (`e2e-tests.yml`) 
3. **Finalization** (`finalize-release.yml`) or **Rollback** (`rollback-release.yml`)

## Detailed Flow

### 1. Initial Trigger
```
git tag v1.0.0
git push origin v1.0.0
```

### 2. Deploy Release (deploy.yml)
**Trigger:** Push of tag `v*`

**Actions:**
- Extract version from tag
- Build packages
- Publish to NPM with `pre-release` tag
- Trigger E2E tests workflow

**Published packages:**
- `@motiadev/core@version` (tag: pre-release)
- `@motiadev/workbench@version` (tag: pre-release)
- `@motiadev/stream-client-browser@version` (tag: pre-release)
- `@motiadev/stream-client-react@version` (tag: pre-release)
- `motia@version` (tag: pre-release)
- `@motiadev/test@version` (tag: pre-release)

### 3. E2E Tests (e2e-tests.yml)
**Trigger:** Triggered by deploy.yml

**Actions:**
- Install pre-release versions
- Execute E2E tests with Playwright
- Upload test artifacts
- Trigger next step based on result

**Scenarios:**
- ✅ **Success:** Triggers `finalize-release.yml`
- ❌ **Failure:** Triggers `rollback-release.yml`

### 4A. Finalization (finalize-release.yml)
**Trigger:** E2E tests passed

**Actions:**
- Promote packages to `latest` tag
- Remove `pre-release` tag
- Generate automatic changelog
- Create GitHub Release
- Commit version changes

### 4B. Rollback (rollback-release.yml)
**Trigger:** E2E tests failed

**Actions:**
- Remove pre-release packages from NPM
- Delete problematic Git tag
- Create failure issue on GitHub
- Download test artifacts for analysis

## Workflow Structure

```
📁 .github/workflows/
├── deploy.yml           # Main workflow
├── e2e-tests.yml       # End-to-end tests
├── finalize-release.yml # Release finalization
├── rollback-release.yml # Rollback in case of failure
├── motia.yml           # CI/CD (quality checks)
└── wip-release.yml     # Manual WIP releases
```

## NPM Tags

### During Deploy
- `pre-release`: Version under test
- `latest`: Stable version (after tests)

### Package States
```bash
# During E2E tests
npm install motia@1.0.0 --tag pre-release

# After successful tests
npm install motia@1.0.0  # latest tag (default)
```

## Logs and Artifacts

### Success
- ✅ GitHub Release created
- ✅ Packages promoted to `latest`
- ✅ Changelog generated automatically

### Failure
- ❌ Issue created automatically
- ❌ Test artifacts preserved
- ❌ Pre-release packages removed
- ❌ Git tag removed

## Verification Commands

### Check deploy status
```bash
# List recent workflows
gh run list

# View workflow details
gh run view <run-id>

# Download test artifacts
gh run download <run-id>
```

### Check NPM packages
```bash
# View available tags
npm view motia dist-tags

# View published versions
npm view motia versions --json
```

### Check GitHub releases
```bash
# List releases
gh release list

# View release details
gh release view v1.0.0
```

## Manual Recovery

### If rollback fails
```bash
# Remove packages manually
npm unpublish motia@1.0.0 --force

# Remove Git tag
git push --delete origin v1.0.0
git tag -d v1.0.0
```

### Restart deploy
```bash
# Create new tag
git tag v1.0.1
git push origin v1.0.1
```

## Required Configuration

### GitHub Secrets
- `MOTIA_CI_APP_ID`: App ID for authentication
- `MOTIA_CI_APP_PRIVATE_KEY`: App private key
- `NPM_TOKEN`: Token for NPM publishing

### Required Permissions
- `contents: write`: For tags and releases
- `packages: write`: For NPM publishing
- `actions: write`: For triggering workflows
- `issues: write`: For creating failure issues

## Troubleshooting

### E2E tests failing consistently
1. Check logs in `e2e-tests.yml` workflow
2. Download test artifacts
3. Run tests locally:
   ```bash
   cd packages/e2e
   pnpm test:e2e
   ```

### NPM publishing issues
1. Check NPM token
2. Check package permissions
3. Check if version already exists

### Workflows not triggering
1. Check GitHub App permissions
2. Check trigger configuration
3. Check workflow logs