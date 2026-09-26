#!/usr/bin/env bash
# Regenerate the committed CLI reference page from the clap definitions.
#
# Each user-facing binary in this repo (iii and iii-console) exposes
# a hidden `gen-cli-docs` subcommand that renders its own clap tree as MDX via
# crates/iii-clap-docs. The engine emits the full page (frontmatter + intro);
# console emits a fragment, concatenated below it as a sibling `##`
# sections of one combined page. The output is committed at
# docs/next/cli-reference/index.mdx and the cli-docs-built CI job regenerates
# + diffs it, so the docs can never drift from the CLI. (iii-cloud lives
# outside this repo and is not covered.)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="docs/next/cli-reference"
OUT_FILE="$OUT_DIR/index.mdx"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "=== CLI Reference Generation ==="

echo "[1/3] iii (engine)..."
cargo run --quiet -p iii -- gen-cli-docs --out "$TMP/iii.mdx"

echo "[2/3] iii console..."
# Placeholder assets are fine; gen-cli-docs never serves the frontend.
SKIP_FRONTEND_BUILD=1 cargo run --quiet -p iii-console -- gen-cli-docs --out "$TMP/iii-console.mdx"

mkdir -p "$OUT_DIR"
# The Telemetry section is hand-authored prose, not a clap tree, so it is
# appended here after both generated fragments to sit at the bottom of the
# page. Keep it in sync with the CLI's opt-out gate
# (iii::workers::telemetry::environment::env_opt_out).
{
  cat "$TMP/iii.mdx"
  echo
  cat "$TMP/iii-console.mdx"
  cat <<'TELEMETRY_MDX'

## Telemetry

The engine sends anonymous usage data by default. This data helps to improve iii and contains no personal information.

To turn the usage data off, do one of these:

- Set `III_TELEMETRY_ENABLED` to `false`, `0`, `no`, or `off` before you start `iii`. Letter case does not matter, and leading or trailing spaces are ignored. Any other value, or no value, keeps the usage data on.
- Create the file `~/.iii/telemetry_dev_optout`. The engine reads this file whenever the process starts.
- Set `telemetry.enabled: false` in the engine configuration.

The engine also turns the usage data off automatically if it detects that it is in a CICD environment.

This setting controls anonymous product-usage data only. It does not change OpenTelemetry observability (traces, metrics, and logs) for your own monitoring of your iii system.
TELEMETRY_MDX
} > "$OUT_FILE"

# Re-render the per-doc skill artifact (<page>.mdx.skill.md) that the
# skill-check workflow verifies. Optional locally; CI's skill-check job is
# the authority.
echo "[3/3] skill artifact..."
if command -v iii-skill-render &>/dev/null; then
  iii-skill-render --write "$OUT_FILE"
else
  echo "  [SKIP] iii-skill-render not found; skill-check CI will report if the artifact is stale"
fi

echo "=== Done: $OUT_FILE ==="
