# HOWTO-to-Skills Sync Automation Plan

Plan for automatically adding, removing, and updating skills when HOWTOs are added, removed, or updated in the iii docs.

## Goal

Keep the skills repository in sync with the HOWTOs at https://iii.dev/docs. When a HOWTO is added, updated, or removed, the corresponding skill should be created, updated, or flagged for removal.

## Current State

The docs index lives at `https://iii.dev/docs/llms.txt` and lists every page with its URL and description. HOWTO pages follow the pattern `https://iii.dev/docs/how-to/{slug}`. Each HOWTO maps to one skill directory in this repo.

## Architecture

```
iii.dev/docs/llms.txt (source of truth)
  ↓
Scheduled agent (weekly cron or GitHub Action)
  ↓
Diff current HOWTOs against current skills
  ↓
Generate/update skill files
  ↓
Open PR for review
```

## Detection: What Changed?

### Data source

Fetch `https://iii.dev/docs/llms.txt` and parse the HOWTO section. Each entry has:
- URL: `https://iii.dev/docs/how-to/{slug}`
- Description: one-line summary

### Diffing strategy

Maintain a `references/howto-index.json` file tracking the last-known state:

```json
{
  "last_synced": "2026-03-23",
  "howtos": {
    "expose-http-endpoint": {
      "url": "https://iii.dev/docs/how-to/expose-http-endpoint",
      "description": "How to register a function and expose it as a REST API endpoint.",
      "skill": "http-endpoints",
      "content_hash": "sha256:abc123..."
    }
  }
}
```

On each run:
1. Fetch `llms.txt` and extract HOWTO entries
2. Fetch each HOWTO page and compute content hash
3. Compare against `howto-index.json`
4. Classify changes: **added**, **updated** (hash changed), **removed** (missing from index)

## Action: What to Do?

### HOWTO Added

1. Derive skill name from slug (e.g., `use-queues` → `queue-processing`)
2. Create skill directory with `SKILL.md` and `references/reference.js`
3. Generate SKILL.md from HOWTO content following the pattern skill template
4. Generate reference.js with working code examples from the HOWTO
5. Update `SKILLS.md` index
6. Update `howto-index.json`

### HOWTO Updated

1. Fetch the updated HOWTO page
2. Diff against the current skill content
3. Regenerate `references/reference.js` with updated code examples
4. Update `SKILL.md` if key concepts, primitives, or patterns changed
5. Update content hash in `howto-index.json`

### HOWTO Removed

1. Flag the skill for manual review (don't auto-delete)
2. Add a `# DEPRECATED` notice to the SKILL.md
3. Open an issue or PR comment asking for human decision
4. Update `howto-index.json`

## Implementation Options

### Option A: GitHub Action with AI Agent

A GitHub Action runs on a schedule (weekly) or on-demand:

1. **Fetch and diff** — Shell script fetches `llms.txt`, compares with `howto-index.json`
2. **Agent generates content** — For each change, invoke an AI agent (Claude API) with:
   - The HOWTO page content as input
   - The pattern skill template as the target format
   - Instructions to produce SKILL.md (no code blocks, no config) and reference.js
3. **Open PR** — Use `peter-evans/create-pull-request` to open a PR with the changes
4. **Human review** — Maintainer reviews the PR before merging

**Pros:** Fully automated, runs without human intervention, produces high-quality content.
**Cons:** Requires Claude API key in GitHub secrets, API costs, may need manual review for accuracy.

### Option B: iii Engine Agent Worker

An iii worker that runs as a scheduled cron function:

1. Register a function `skills::sync-howtos` that:
   - Fetches `llms.txt` via HTTP
   - Diffs against stored state
   - Generates skill content
   - Commits and pushes changes (or opens a PR via GitHub API)
2. Register a cron trigger: `0 0 0 * * 1 *` (weekly Monday midnight)

**Pros:** Dog-foods the iii platform, demonstrates cron + state + HTTP patterns.
**Cons:** Requires a running iii instance, more complex deployment.

### Option C: Manual with AI Assist

No automation. When docs change, a maintainer:

1. Runs a local script: `./scripts/sync-howtos.sh`
2. Script fetches `llms.txt`, diffs, and prints what changed
3. Maintainer uses Claude Code to generate/update skills
4. Maintainer commits and pushes

**Pros:** Simple, no CI/CD dependency, full human control.
**Cons:** Relies on maintainer discipline, easy to forget.

## Recommended Approach

**Option A (GitHub Action with AI Agent)** is recommended because:

- Runs automatically without human memory
- Produces consistent, template-compliant output
- Human review via PR ensures quality
- The `sync-config.yml` workflow already demonstrates the pattern

### Implementation Steps

1. Create `scripts/diff-howtos.js` — fetches `llms.txt`, diffs against `howto-index.json`, outputs changes
2. Create `scripts/generate-skill.js` — given a HOWTO URL, generates SKILL.md and reference.js
3. Create `.github/workflows/sync-howtos.yml` — scheduled workflow that runs diff, generates, and opens PR
4. Create `references/howto-index.json` — initial state mapping HOWTOs to skills
5. Add `CLAUDE_API_KEY` to GitHub repository secrets

### Mapping HOWTOs to Skills

| HOWTO Slug | Skill Name |
|------------|------------|
| `expose-http-endpoint` | `http-endpoints` |
| `schedule-cron-task` | `cron-scheduling` |
| `use-queues` | `queue-processing` |
| `manage-state` | `state-management` |
| `react-to-state-changes` | `state-reactions` |
| `stream-realtime-data` | `realtime-streams` |
| `create-custom-trigger-type` | `custom-triggers` |
| `use-functions-and-triggers` | `functions-and-triggers` |
| `trigger-actions` | `trigger-actions` |
| `use-trigger-conditions` | `trigger-conditions` |
| `dead-letter-queues` | `dead-letter-queues` |
| `configure-engine` | `engine-config` |

HOWTOs without a skill mapping (too niche):
- `trigger-functions-from-cli` — single CLI command
- `create-ephemeral-worker` — niche, could be folded into `functions-and-triggers`

Non-HOWTO docs that map to skills:
- `advanced/telemetry` → `observability`
- `api-reference/sdk-node` → `node-sdk`
- `api-reference/sdk-python` → `python-sdk`
- `api-reference/sdk-rust` → `rust-sdk`
