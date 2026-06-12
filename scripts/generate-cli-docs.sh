#!/usr/bin/env bash
# Regenerate the committed CLI reference page from the clap definitions.
#
# Each user-facing binary in this repo (iii, iii-worker, iii-console) exposes
# a hidden `gen-cli-docs` subcommand that renders its own clap tree as MDX via
# crates/iii-clap-docs. The engine emits the full page (frontmatter + intro);
# worker and console emit fragments, concatenated below it as sibling `##`
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

echo "[1/4] iii (engine)..."
cargo run --quiet -p iii -- gen-cli-docs --out "$TMP/iii.mdx"

echo "[2/4] iii worker..."
cargo run --quiet -p iii-worker -- gen-cli-docs --out "$TMP/iii-worker.mdx"

echo "[3/4] iii console..."
# Placeholder assets are fine; gen-cli-docs never serves the frontend.
SKIP_FRONTEND_BUILD=1 cargo run --quiet -p iii-console -- gen-cli-docs --out "$TMP/iii-console.mdx"

mkdir -p "$OUT_DIR"
{
  cat "$TMP/iii.mdx"
  echo
  cat "$TMP/iii-worker.mdx"
  echo
  cat "$TMP/iii-console.mdx"
} > "$OUT_FILE"

# Re-render the per-doc skill artifact (<page>.mdx.skill.md) that the
# skill-check workflow verifies. Optional locally; CI's skill-check job is
# the authority.
echo "[4/4] skill artifact..."
if command -v iii-skill-render &>/dev/null; then
  iii-skill-render --write "$OUT_FILE"
else
  echo "  [SKIP] iii-skill-render not found; skill-check CI will report if the artifact is stale"
fi

echo "=== Done: $OUT_FILE ==="
