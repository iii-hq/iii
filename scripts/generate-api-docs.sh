#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== SDK API Docs Generation Pipeline ==="
echo ""

# Step 1: Node SDK (TypeDoc)
echo "[1/4] Extracting Node SDK docs (TypeDoc)..."
pnpm --filter iii-sdk docs:json
echo "  Done: sdk/packages/node/iii/api-docs.json"

# Step 2: Python SDK (griffe)
echo "[2/4] Extracting Python SDK docs (griffe)..."
cd sdk/packages/python/iii
if command -v uv &>/dev/null; then
  uv sync --extra dev --quiet
  uv run griffe dump iii -d google > api-docs.json
else
  echo "  [SKIP] uv not found. Install uv or run: pip install griffe && griffe dump iii --docstring-parser google > api-docs.json"
fi
cd "$REPO_ROOT"
echo "  Done: sdk/packages/python/iii/api-docs.json"

# Step 3: Rust SDK (nightly rustdoc JSON)
echo "[3/4] Extracting Rust SDK docs (rustdoc JSON)..."
if rustup toolchain list | grep -q nightly; then
  cargo +nightly rustdoc -p iii-sdk --all-features -- --output-format json 2>/dev/null || \
    echo "  [WARN] rustdoc JSON generation failed (nightly may be incompatible)"
else
  echo "  [SKIP] Rust nightly not installed. Run: rustup toolchain install nightly"
fi

# Step 4: Generate MDX
echo "[4/4] Generating MDX files..."
pnpm tsx docs/scripts/generate-api-docs.mts

echo ""
echo "=== Done ==="
