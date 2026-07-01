# Releasing docs (Next → Latest)

This is the contributor runbook for how documentation versions move through the site. The docs use a
custom Next / Latest / archive scheme: you author upcoming docs in `docs/next/`, and a stable release
promotes that folder into the published **Latest** while freezing the previous Latest into a numbered
archive. This file is the operator-facing companion to the source of truth — the
[`create-tag.yml` section of `WORKFLOWS.md`](../.github/workflows/WORKFLOWS.md) and the
[`pin_docs.py`](../.github/scripts/pin_docs.py) rotation script.

## Folder layout

| State | Location | Paths in `docs.json` |
|-------|----------|----------------------|
| **Latest** (current stable) | `docs/` root | unprefixed (`index.mdx`, `using-iii/...`) |
| **Next** (in-progress) | fixed `docs/next/` folder | `next/`-prefixed |
| **Archived** (frozen) | `docs/MAJOR-MINOR-0/` (e.g. `docs/0-19-0/`) | `0-19-0/`-prefixed, no tag |
| **Shared** | `docs/changelog/` | stays at root, never moved |

In-content links are version-relative, so files move verbatim during a release. Only the
`navigation.versions` nav paths in `docs/docs.json` carry the version prefix.

## Before a release — prepare `docs/next/`

All documentation for the upcoming version is authored under `docs/next/`. A stable release pulls
whatever is in that folder, so it must be release-ready before you cut the tag.

Two generated trees must be regenerated and committed into `docs/next/` first:

| Generated docs | Command | Output |
|----------------|---------|--------|
| CLI reference | `make cli-docs` (or `./scripts/generate-cli-docs.sh`) | `docs/next/cli-reference/index.mdx` |
| API / SDK reference | `pnpm tsx docs/next/scripts/generate-api-docs.mts` | `docs/next/api-reference/*.mdx` |

CI gates drift: the `cli-docs-built` job in `.github/workflows/ci.yml` fails if
`docs/next/cli-reference/` is stale relative to the CLI definitions.

## Cut a release

Releases are driven entirely by the **Create Tag** workflow (`.github/workflows/create-tag.yml`,
manual `workflow_dispatch`). There is no standalone promote script — trigger the workflow from the
Actions tab with:

| Input | Value |
|-------|-------|
| `target` | `iii` |
| `bump` | `patch`, `minor`, or `major` |
| `prerelease` | `none` — **must be `none` or the docs do not rotate** |
| `dry_run` | `true` for a rehearsal first |

The workflow validates the docs, rotates them, then commits, tags (`iii/v{version}`), and pushes as
`iii-ci[bot]`.

### The validate gate

Stable releases run `pin_docs.py validate` first (even on dry runs) and abort with a Slack alert
unless all of the following hold:

- `docs/docs.json` has a `Next` version block, and
- `docs/next/` is non-empty, and
- `docs/next/cli-reference/index.mdx` is non-empty.

## What rotation does

`pin_docs.py rotate` dispatches on the bump type. For the full mechanics see the
[`create-tag.yml` section of `WORKFLOWS.md`](../.github/workflows/WORKFLOWS.md) and
[`pin_docs.py`](../.github/scripts/pin_docs.py); in summary:

- **minor / major** — archive the old Latest (root) into `docs/OLD-MINOR-0/`, promote `docs/next/`
  into the root as the new **Latest** (labeled with the tag version), relabel the **Next** block to
  `minor + 1` (the `docs/next/` folder stays in place), and reorder the version dropdown.
- **patch** — refresh the root from `docs/next/` in place. No archive, no Next bump, no version-block
  changes.
- **prerelease** (`alpha` / `beta` / `rc` / `next`) — docs are left untouched.

## Verify locally before tagging

```bash
# Same gate the workflow runs — should exit 0.
python3 .github/scripts/pin_docs.py validate --docs-dir docs

# Preview the Next docs in the version dropdown.
pnpm dev:docs
```
