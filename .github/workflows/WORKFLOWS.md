# CI/CD Workflows

## Overview

The workflows are organized into two categories:

- **Top-level workflows** — triggered by events (push, PR, tag, manual dispatch)
- **Reusable workflows** — prefixed with `_`, called by top-level workflows via `workflow_call`

```text
                ┌──────────────┐
                │  create-tag  │  (manual dispatch)
                └──────┬───────┘
                       │ creates git tag
                       ▼
        ┌──────────────────────────────┐
        │      Tag push triggers       │
        ├──────────────┬───────────────┤
        │  iii/v*      │  motia/v*     │
        ▼              ▼               │
   release-iii    release-motia        │
        │              │               │
        │   ┌──────────┴──────┐        │
        │   │  _npm.yml       │        │
        │   │  _py.yml        │        │
        │   │  _rust-cargo.yml│        │
        │   │  _rust-binary.yml│       │
        │   │  _homebrew.yml  │        │
        │   └─────────────────┘        │
        │     (reusable workflows)     │
        └──────────────────────────────┘

   ci.yml ◄── push to main / PRs
   docker-engine.yml ◄── called by release-iii / manual
   license-check.yml ◄── push to main / PRs
```

---

## Top-Level Workflows

### `ci.yml` — Continuous Integration

**Triggers:** push to `main`, pull requests to `main`, manual dispatch

Runs the full test suite across the monorepo. Cancels in-progress runs for PRs.

| Job | Depends On | What it does |
|-----|-----------|--------------|
| `build-engine` | — | Fmt check, tests, release build. Uploads `iii-binary` artifact |
| `engine-build-matrix` | — | Cross-platform build validation (macOS, Windows, Linux, musl) |
| `sdk-node-ci` | `build-engine` | Type check, build, start engine, run SDK tests |
| `sdk-python-ci` | `build-engine` | Lint (ruff), type check (mypy), start engine, run pytest. Matrix: Python 3.10/3.11/3.12 |
| `sdk-rust-ci` | `build-engine` | Fmt, clippy, start engine, run cargo tests |
| `motia-js-ci` | `build-engine`, `sdk-node-ci` | Build SDK + Motia, start engine, run tests |
| `motia-py-ci` | `build-engine`, `sdk-python-ci` | Lint, type check, start engine, run pytest. Matrix: Python 3.11/3.12/3.13 |
| `console-ci` | `build-engine` | Lint + build frontend (Node 22), build console Rust binary |

All SDK and framework tests download the engine binary artifact and start a live engine instance before running.

---

### `create-tag.yml` — Version Bump & Tag Creation

**Triggers:** manual dispatch only

Entry point for all releases. Provides a form with:

| Input | Options |
|-------|---------|
| `target` | `iii` or `motia` |
| `bump` | `patch`, `minor`, `major` |
| `prerelease` | `none`, `alpha`, `beta`, `rc` |
| `dry_run` | boolean |

**What it does:**

1. Validates it's running on `main` and all required manifest files exist
2. Reads the current version from the canonical manifest
3. Calculates the next version (handles semver bump + prerelease labels + dry-run suffixes)
4. Converts to PEP 440 format for Python packages (e.g., `1.0.0-alpha.1` becomes `1.0.0a1`)
5. Updates all manifest files in lockstep (Cargo.toml, package.json, pyproject.toml)
6. Commits the version bump, creates an annotated tag, and pushes both
7. Posts a Slack notification

The tag push then triggers the corresponding release workflow.

**Tag format:** `{target}/v{version}` (e.g., `iii/v1.2.3`, `motia/v0.5.0-beta.1`)

---

### `release-iii.yml` — iii Release Pipeline

**Triggers:** tag push matching `iii/v*`

Orchestrates the full iii release across all package registries and distribution channels.

```text
setup (parse tag metadata, Slack notification)
  │
  ├─► create-iii-release (GitHub Release with auto-generated notes)
  │     │
  │     ├─► engine-release ──────► _rust-binary.yml (9 platform targets)
  │     │     │
  │     │     ├─► docker ────────► docker-engine.yml (pre-built binaries, no compilation)
  │     │     │
  │     │     └─► homebrew-engine ► _homebrew.yml (stable only)
  │     │
  │     ├─► console-frontend ───► Build React frontend for embedding
  │     │     │
  │     │     └─► console-release ► _rust-binary.yml (with embedded frontend)
  │     │           │
  │     │           └─► homebrew-console ► _homebrew.yml (stable only)
  │     │
  │     ├─► sdk-npm ─────────────► _npm.yml
  │     ├─► sdk-py ──────────────► _py.yml
  │     └─► sdk-rust ────────────► _rust-cargo.yml
  │
  └─► notify-complete (aggregated Slack status)
```

**Setup job** parses the tag to determine:
- `version` — stripped prefix (e.g., `iii/v1.2.3` becomes `1.2.3`)
- `is_prerelease` — true if version contains a prerelease label
- `npm_tag` — dist-tag for npm (`latest`, `alpha`, `beta`, `rc`, `dry-run`)
- `dry_run` — true if version ends with `-dry-run.N`

**Concurrency:** only one iii release runs at a time per repository.

**Skipped on dry run:** GitHub Release creation, Homebrew publish.

---

### `release-motia.yml` — Motia Release Pipeline

**Triggers:** tag push matching `motia/v*`

Simpler than iii — publishes only the Motia framework packages.

```text
setup (parse tag metadata, Slack notification)
  │
  ├─► motia-npm ──► _npm.yml (builds iii-sdk first as dependency)
  ├─► motia-py ───► _py.yml
  │
  └─► notify-complete (aggregated Slack status)
```

---

### `docker-engine.yml` — Docker Image Build & Publish

**Triggers:** called by `release-iii.yml` after engine binaries are built, or manual dispatch with a release tag

Downloads pre-built binaries from the GitHub Release (no Rust compilation) and packages them into a minimal distroless Docker image.

| Job | Runner | What it does |
|-----|--------|--------------|
| `setup` | `ubuntu-latest` | Parse version from release tag |
| `build` (amd64) | `ubuntu-latest` | Download pre-built binary, build + push image |
| `build` (arm64) | `ubuntu-24.04-arm` | Download pre-built binary, build + push image (native ARM runner) |
| `publish` | `ubuntu-latest` | Create multi-platform manifest, Trivy security scan, push to GHCR + DockerHub |

**Registries:** GHCR (`ghcr.io`) and DockerHub (`iiidev/iii`)

**Security:** Trivy vulnerability scanning (CRITICAL + HIGH), distroless nonroot runtime.

---

### `license-check.yml` — License Header Check

**Triggers:** push to `main`, pull requests to `main`

Uses [hawkeye](https://github.com/korandoru/hawkeye) to verify license headers across source files, configured via `engine/licenserc.toml`.

---

## Reusable Workflows

All reusable workflows support `dry_run` mode and Slack thread notifications.

### `_npm.yml` — NPM Publish

Publishes a Node.js package to the npm registry.

| Input | Purpose |
|-------|---------|
| `package_path` | Directory containing the package to publish |
| `npm_tag` | dist-tag (`latest`, `alpha`, `beta`, `rc`) |
| `build_filter` | pnpm filter for building the package |
| `pre_build_filter` | pnpm filter for building dependencies first (optional) |

Uses `pnpm publish` with `--no-git-checks` and `--access public`.

### `_py.yml` — PyPI Publish

Publishes a Python package to PyPI.

Builds with `python -m build`, validates with `twine check` on dry run, publishes via `pypa/gh-action-pypi-publish`.

### `_rust-cargo.yml` — Cargo Publish

Publishes a Rust crate to crates.io via `cargo publish`.

### `_rust-binary.yml` — Rust Binary Release

Cross-compiles a Rust binary for 9 platform targets and uploads them to a GitHub Release.

**Targets:**

| Platform | Runner |
|----------|--------|
| `x86_64-apple-darwin` | `macos-latest` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-pc-windows-msvc` | `windows-latest` |
| `i686-pc-windows-msvc` | `windows-latest` |
| `aarch64-pc-windows-msvc` | `windows-latest` |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04` |
| `armv7-unknown-linux-gnueabihf` | `ubuntu-22.04` |

Supports downloading a pre-built artifact (used by console to embed the frontend build).

Uses `taiki-e/upload-rust-binary-action` for building and uploading.

### `_homebrew.yml` — Homebrew Formula Publish

Generates and publishes a Homebrew formula to the `iii-hq/homebrew-tap` repository.

1. Downloads release tarballs from GitHub Releases
2. Calculates SHA256 checksums
3. Generates a Ruby formula file with platform-specific URLs
4. Tests the formula locally (`brew audit`, `brew install`, version check)
5. Commits and pushes to the tap repository

Only runs for stable (non-prerelease) versions.

---

## Secrets

| Secret | Used by |
|--------|---------|
| `III_CI_APP_ID` / `III_CI_APP_PRIVATE_KEY` | GitHub App token for pushing tags, creating releases, updating homebrew-tap |
| `NPM_TOKEN` | npm registry authentication |
| `PYPI_API_TOKEN` / `PYPI_MOTIA_TOKEN` | PyPI publishing (separate tokens for iii and motia) |
| `CARGO_REGISTRY_TOKEN` | crates.io publishing |
| `DOCKERHUB_USERNAME` / `DOCKERHUB_PASSWORD` | DockerHub publishing |
| `SLACK_BOT_TOKEN` / `SLACK_CHANNEL_ID` | Slack release notifications |
| `SLACK_WEBHOOK_URL` | Slack Docker notifications |

---

## Release Flow (End to End)

1. Developer triggers `create-tag` workflow manually, selecting target/bump/prerelease
2. Workflow bumps versions across all manifests, commits, and pushes a tag
3. Tag push triggers the corresponding release workflow (`release-iii` or `release-motia`)
4. Release workflow fans out to reusable workflows in parallel
5. Each reusable workflow posts progress to a Slack thread
6. Final job aggregates results and updates the parent Slack message with overall status
