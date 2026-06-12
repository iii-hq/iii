#!/usr/bin/env bash
# Regenerate the committed CLI reference pages from the clap definitions.
#
# Each user-facing binary in this repo (iii, iii-worker, iii-console) exposes
# a hidden `gen-cli-docs` subcommand that renders its own clap tree as MDX via
# crates/iii-clap-docs. The output is committed under docs/next/cli-reference/
# and the cli-docs-built CI job regenerates + diffs it, so the docs can never
# drift from the CLI. (iii-cloud lives outside this repo and is not covered.)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="docs/next/cli-reference"

echo "=== CLI Reference Generation ==="

echo "[1/4] iii (engine)..."
cargo run --quiet -p iii -- gen-cli-docs --out "$OUT_DIR/iii.mdx"

echo "[2/4] iii worker..."
cargo run --quiet -p iii-worker -- gen-cli-docs --out "$OUT_DIR/iii-worker.mdx"

echo "[3/4] iii console..."
# Placeholder assets are fine; gen-cli-docs never serves the frontend.
SKIP_FRONTEND_BUILD=1 cargo run --quiet -p iii-console -- gen-cli-docs --out "$OUT_DIR/iii-console.mdx"

# Re-render the per-doc skill artifacts (<page>.mdx.skill.md) that the
# skill-check workflow verifies. Optional locally; CI's skill-check job is
# the authority.
echo "[4/4] skill artifacts..."
if command -v iii-skill-render &>/dev/null; then
  # Single-file mode: directory targets require a worker/docs-root layout
  # this folder doesn't have.
  for page in "$OUT_DIR"/*.mdx; do
    iii-skill-render --write "$page"
  done
else
  echo "  [SKIP] iii-skill-render not found; skill-check CI will report if artifacts are stale"
fi

echo "=== Done: $OUT_DIR ==="
